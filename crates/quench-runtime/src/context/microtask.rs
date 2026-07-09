//! Microtask queue for Promise resolution.

use std::collections::VecDeque;

use crate::value::Value;

/// Queue for Promise microtasks.
pub struct MicrotaskQueue {
    queue: VecDeque<Value>,
}

impl MicrotaskQueue {
    pub fn new() -> Self {
        MicrotaskQueue { queue: VecDeque::new() }
    }

    pub fn enqueue(&mut self, task: Value) {
        self.queue.push_back(task);
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn dequeue(&mut self) -> Option<Value> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

impl Default for MicrotaskQueue {
    fn default() -> Self {
        Self::new()
    }
}
