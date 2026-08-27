//! Host-side event loop queues: microtasks and immediates.
//!
//! The pump drains these between timer phases; `process.nextTick`
//! and timer callbacks enqueue into them.

use std::cell::RefCell;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

pub struct EventLoop {
    pub microtasks: RefCell<Vec<Microtask>>,
    pub immediates: RefCell<Vec<(Value, Vec<Value>)>>,
}

pub struct Microtask {
    pub callback: Value,
    pub args: Vec<Value>,
    pub resource: Option<Value>,
    pub domain: Option<Value>,
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
        }
    }

    pub fn queue_microtask(&self, cb: Value, args: Vec<Value>) {
        self.queue_microtask_with_resource(cb, args, None);
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
            resource,
            domain: None,
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
            resource,
            domain,
        });
    }

    pub fn queue_immediate(&self, cb: Value, args: Vec<Value>) {
        self.immediates.borrow_mut().push((cb, args));
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
            for (cb, args) in snapshot {
                let _ = call(&cb, &args);
            }
        }
    }
}
