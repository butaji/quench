//! Host-side event loop queues: microtasks and immediates.
//!
//! The pump drains these between timer phases; `process.nextTick`
//! and timer callbacks enqueue into them.

use std::cell::{Cell, RefCell};

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub struct EventLoop {
    pub microtasks: RefCell<Vec<Microtask>>,
    pub immediates: RefCell<Vec<Immediate>>,
    process_scope: Cell<u64>,
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
            process_scope: Cell::new(0),
        }
    }

    pub fn process_scope(&self) -> u64 {
        self.process_scope.get()
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
