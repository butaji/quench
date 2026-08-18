//! `timers` module + microtask/immediate/timer wheel.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub trait TimerHandler: FnMut(&Value, &[Value]) -> Result<Value, VmError> {}

pub struct TimerRegistry {
    pub next_id: u64,
    pub timers: HashMap<u64, Timer>,
    pub tick_at: u64,
}

pub enum Timer {
    Timeout { fire_at: u64 },
    Interval { fire_at: u64, period: u64 },
    Immediate,
}

impl TimerRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            timers: HashMap::new(),
            tick_at: 0,
        }
    }

    pub fn next(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

pub fn set_timeout(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let cb = args.first().cloned().unwrap_or(Value::Undefined);
    let delay_ms = args.get(1).map(value_to_u64).unwrap_or(0);
    let id = state.borrow_mut().timers.next();
    let fire_at = monotonic_ms().saturating_add(delay_ms);
    state
        .borrow_mut()
        .timers
        .timers
        .insert(id, Timer::Timeout { fire_at });
    state
        .borrow_mut()
        .event_loop
        .queue_immediate(cb, vec![Value::Number(id as f64)]);
    Ok(Value::Number(id as f64))
}

pub fn clear_timeout(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let Some(id) = args.first().and_then(value_to_id) {
        state.borrow_mut().timers.timers.remove(&id);
    }
    Ok(Value::Undefined)
}

pub fn set_interval(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let cb = args.first().cloned().unwrap_or(Value::Undefined);
    let delay_ms = args.get(1).map(value_to_u64).unwrap_or(0);
    let id = state.borrow_mut().timers.next();
    let fire_at = monotonic_ms().saturating_add(delay_ms);
    state.borrow_mut().timers.timers.insert(
        id,
        Timer::Interval {
            fire_at,
            period: delay_ms,
        },
    );
    state
        .borrow_mut()
        .event_loop
        .queue_immediate(cb, vec![Value::Number(id as f64)]);
    Ok(Value::Number(id as f64))
}

pub fn clear_interval(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    clear_timeout(state, args)
}

pub fn set_immediate(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let cb = args.first().cloned().unwrap_or(Value::Undefined);
    let rest = args.get(1..).unwrap_or(&[]).to_vec();
    let id = state.borrow_mut().timers.next();
    state
        .borrow_mut()
        .timers
        .timers
        .insert(id, Timer::Immediate);
    state.borrow_mut().event_loop.queue_immediate(cb, rest);
    Ok(Value::Number(id as f64))
}

pub fn clear_immediate(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    clear_timeout(state, args)
}

pub fn tick(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

fn monotonic_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn value_to_u64(value: &Value) -> u64 {
    match value {
        Value::Number(n) => n.max(0.0) as u64,
        _ => 0,
    }
}

fn value_to_id(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => Some(n.max(0.0) as u64),
        _ => None,
    }
}

/// Build the `timers` namespace bindings.
pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "setTimeout".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_SETTIMEOUT),
        ),
        (
            "clearTimeout".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_CLEARTIMEOUT),
        ),
        (
            "setInterval".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_SETINTERVAL),
        ),
        (
            "clearInterval".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_CLEARINTERVAL),
        ),
        (
            "setImmediate".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_SETIMMEDIATE),
        ),
        (
            "clearImmediate".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_CLEARIMMEDIATE),
        ),
    ]
}
