//! Host-side event loop queues: microtasks and immediates.
//!
//! The pump drains these between timer phases; `process.nextTick`
//! and timer callbacks enqueue into them.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub struct EventLoop {
    pub microtasks: RefCell<Vec<Microtask>>,
    pub immediates: RefCell<Vec<Immediate>>,
    shared: SharedCallbacks,
    process_scope: Cell<u64>,
}

/// Guest callbacks retained by the shared VM host between execution phases.
/// Legacy callbacks continue to use `Value` in the legacy queues above.
pub struct SharedCallback {
    pub callback: rqj::RootId,
    pub receiver: rqj::RootId,
    pub args: Vec<rqj::RootId>,
}

struct SharedCallbacks {
    next_ticks: VecDeque<SharedCallback>,
    immediates: VecDeque<SharedImmediate>,
    next_immediate_id: u64,
    timers: VecDeque<SharedTimer>,
    next_timer_id: u64,
    firing_timers: HashSet<u64>,
    cancelled_timers: HashSet<u64>,
    listeners: HashMap<String, Vec<SharedCallback>>,
    exiting: bool,
}

pub struct SharedImmediate {
    pub id: u64,
    pub callback: SharedCallback,
}

pub struct SharedTimer {
    pub id: u64,
    pub due: std::time::Instant,
    pub interval: Option<std::time::Duration>,
    pub callback: SharedCallback,
}

const FIRST_SHARED_IMMEDIATE_ID: u64 = 1;

impl Default for SharedCallbacks {
    fn default() -> Self {
        Self {
            next_ticks: VecDeque::new(),
            immediates: VecDeque::new(),
            next_immediate_id: FIRST_SHARED_IMMEDIATE_ID,
            timers: VecDeque::new(),
            next_timer_id: FIRST_SHARED_IMMEDIATE_ID,
            firing_timers: HashSet::new(),
            cancelled_timers: HashSet::new(),
            listeners: HashMap::new(),
            exiting: false,
        }
    }
}

pub struct Immediate {
    pub callback: Value,
    pub args: Vec<Value>,
    pub resource: Option<Value>,
}

pub struct Microtask {
    pub callback: Value,
    pub args: Vec<Value>,
    pub receiver: Option<Value>,
    pub resource: Option<Value>,
    pub domain: Option<Value>,
    pub domain_stack: Option<Vec<Value>>,
    pub process_scope: u64,
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLoop {
    pub fn new() -> Self {
        Self {
            microtasks: RefCell::new(Vec::new()),
            immediates: RefCell::new(Vec::new()),
            shared: SharedCallbacks::default(),
            process_scope: Cell::new(0),
        }
    }

    pub fn queue_shared_next_tick(&mut self, callback: SharedCallback) {
        if !self.shared.exiting {
            self.shared.next_ticks.push_back(callback);
        }
    }

    pub fn reserve_shared_immediate_id(&mut self) -> Option<u64> {
        let id = self.shared.next_immediate_id;
        self.shared.next_immediate_id = id.checked_add(1)?;
        Some(id)
    }

    pub fn queue_shared_immediate(&mut self, id: u64, callback: SharedCallback) {
        self.shared
            .immediates
            .push_back(SharedImmediate { id, callback });
    }

    pub fn shared_immediate_cutoff(&self) -> Option<u64> {
        self.shared.next_immediate_id.checked_sub(1)
    }

    pub fn take_shared_immediate_through(&mut self, cutoff: u64) -> Option<SharedImmediate> {
        let index = self
            .shared
            .immediates
            .iter()
            .position(|immediate| immediate.id <= cutoff)?;
        self.shared.immediates.remove(index)
    }

    pub fn cancel_shared_immediate(&mut self, id: u64) -> Option<SharedCallback> {
        let index = self
            .shared
            .immediates
            .iter()
            .position(|item| item.id == id)?;
        self.shared
            .immediates
            .remove(index)
            .map(|item| item.callback)
    }

    pub fn reserve_shared_timer_id(&mut self) -> Option<u64> {
        let id = self.shared.next_timer_id;
        self.shared.next_timer_id = id.checked_add(1)?;
        Some(id)
    }

    pub fn queue_shared_timer(
        &mut self,
        id: u64,
        delay: std::time::Duration,
        interval: Option<std::time::Duration>,
        callback: SharedCallback,
    ) {
        self.shared.timers.push_back(SharedTimer {
            id,
            due: std::time::Instant::now() + delay,
            interval,
            callback,
        });
    }

    pub fn take_due_shared_timer(&mut self, now: std::time::Instant) -> Option<SharedTimer> {
        let index = self
            .shared
            .timers
            .iter()
            .enumerate()
            .filter(|(_, timer)| timer.due <= now)
            .min_by_key(|(_, timer)| timer.due)
            .map(|(index, _)| index)?;
        let timer = self.shared.timers.remove(index)?;
        self.shared.firing_timers.insert(timer.id);
        Some(timer)
    }

    pub fn next_shared_timer_due(&self) -> Option<std::time::Instant> {
        self.shared.timers.iter().map(|timer| timer.due).min()
    }

    pub fn cancel_shared_timer(&mut self, id: u64) -> Option<SharedCallback> {
        if self.shared.firing_timers.contains(&id) {
            self.shared.cancelled_timers.insert(id);
            return None;
        }
        let index = self.shared.timers.iter().position(|timer| timer.id == id)?;
        self.shared.timers.remove(index).map(|timer| timer.callback)
    }

    pub fn complete_shared_timer(
        &mut self,
        timer: SharedTimer,
        completed_at: std::time::Instant,
    ) -> Option<SharedCallback> {
        self.shared.firing_timers.remove(&timer.id);
        if self.shared.cancelled_timers.remove(&timer.id) {
            return Some(timer.callback);
        }
        let Some(interval) = timer.interval else {
            return Some(timer.callback);
        };
        let Some(next_due) = completed_at.checked_add(interval) else {
            return Some(timer.callback);
        };
        self.shared.timers.push_back(SharedTimer {
            id: timer.id,
            due: next_due,
            interval: Some(interval),
            callback: timer.callback,
        });
        None
    }

    pub fn add_shared_listener(&mut self, event: String, callback: SharedCallback) {
        self.shared
            .listeners
            .entry(event)
            .or_default()
            .push(callback);
    }

    pub fn take_shared_next_tick(&mut self) -> Option<SharedCallback> {
        self.shared.next_ticks.pop_front()
    }

    pub fn has_shared_next_ticks(&self) -> bool {
        !self.shared.next_ticks.is_empty()
    }

    pub fn shared_is_exiting(&self) -> bool {
        self.shared.exiting
    }

    pub fn begin_shared_exit(&mut self) -> Vec<SharedCallback> {
        if self.shared.exiting {
            return Vec::new();
        }
        self.shared.exiting = true;
        self.shared.listeners.remove("exit").unwrap_or_default()
    }

    pub fn process_scope(&self) -> u64 {
        self.process_scope.get()
    }

    pub fn reset_shared(&mut self) {
        self.shared = SharedCallbacks::default();
    }

    pub fn set_process_scope(&self, scope: u64) {
        self.process_scope.set(scope);
    }

    pub fn queue_microtask(&self, cb: Value, args: Vec<Value>) {
        self.queue_microtask_with_resource(cb, args, None);
    }

    pub fn queue_microtask_scope(&self, cb: Value, args: Vec<Value>, process_scope: u64) {
        self.microtasks.borrow_mut().push(Microtask {
            callback: cb,
            args,
            receiver: None,
            resource: None,
            domain: None,
            domain_stack: None,
            process_scope,
        });
    }

    pub fn queue_microtask_with_resource(
        &self,
        cb: Value,
        args: Vec<Value>,
        resource: Option<Value>,
    ) {
        self.microtasks.borrow_mut().push(Microtask {
            callback: cb,
            args,
            receiver: None,
            resource,
            domain: None,
            domain_stack: None,
            process_scope: self.process_scope.get(),
        });
    }

    /// Queue a callback with an explicit JavaScript receiver. Most jobs use
    /// the normal `undefined` receiver; host-originated events retain the
    /// emitter identity here instead of emulating it through another object.
    pub fn queue_microtask_with_receiver(&self, cb: Value, args: Vec<Value>, receiver: Value) {
        self.queue_microtask_with_receiver_scope(cb, args, receiver, self.process_scope.get());
    }

    pub fn queue_microtask_with_receiver_scope(
        &self,
        cb: Value,
        args: Vec<Value>,
        receiver: Value,
        process_scope: u64,
    ) {
        self.microtasks.borrow_mut().push(Microtask {
            callback: cb,
            args,
            receiver: Some(receiver),
            resource: None,
            domain: None,
            domain_stack: None,
            process_scope,
        });
    }

    pub fn queue_microtask_with_resource_domain(
        &self,
        cb: Value,
        args: Vec<Value>,
        resource: Option<Value>,
        domain: Option<Value>,
    ) {
        self.microtasks.borrow_mut().push(Microtask {
            callback: cb,
            args,
            receiver: None,
            resource,
            domain,
            domain_stack: None,
            process_scope: self.process_scope.get(),
        });
    }

    pub fn queue_microtask_with_domain_stack(
        &self,
        cb: Value,
        args: Vec<Value>,
        resource: Option<Value>,
        stack: Vec<Value>,
    ) {
        self.microtasks.borrow_mut().push(Microtask {
            callback: cb,
            args,
            receiver: None,
            resource,
            domain: stack.last().cloned(),
            domain_stack: Some(stack),
            process_scope: self.process_scope.get(),
        });
    }

    pub fn queue_microtask_with_domain_stack_scope(
        &self,
        cb: Value,
        args: Vec<Value>,
        resource: Option<Value>,
        stack: Vec<Value>,
        process_scope: u64,
    ) {
        self.microtasks.borrow_mut().push(Microtask {
            callback: cb,
            args,
            receiver: None,
            resource,
            domain: stack.last().cloned(),
            domain_stack: Some(stack),
            process_scope,
        });
    }

    pub fn queue_immediate(&self, cb: Value, args: Vec<Value>) {
        self.queue_immediate_with_resource(cb, args, None);
    }

    pub fn queue_immediate_with_resource(
        &self,
        cb: Value,
        args: Vec<Value>,
        resource: Option<Value>,
    ) {
        self.immediates.borrow_mut().push(Immediate {
            callback: cb,
            args,
            resource,
        });
    }

    pub fn drain_microtasks<F>(&self, mut call: F)
    where
        F: FnMut(&Value, &[Value]) -> Result<Value, VmError>,
    {
        loop {
            let snapshot: Vec<_> = self.microtasks.borrow_mut().drain(..).collect();
            if snapshot.is_empty() {
                break;
            }
            for task in snapshot {
                let _ = call(&task.callback, &task.args);
            }
        }
    }

    pub fn drain_immediates<F>(&self, mut call: F)
    where
        F: FnMut(&Value, &[Value]) -> Result<Value, VmError>,
    {
        loop {
            let snapshot: Vec<_> = self.immediates.borrow_mut().drain(..).collect();
            if snapshot.is_empty() {
                break;
            }
            for task in snapshot {
                let _ = call(&task.callback, &task.args);
            }
        }
    }
}
