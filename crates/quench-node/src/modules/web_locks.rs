//! Rust-owned Web Locks state machine.
//!
//! Each realm owns one lock table. Requests are queued by name, and the
//! callback is the only edge that can hold a grant open: a returned Promise
//! closes the grant from its fulfillment/rejection reaction.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::{PromiseData, PromiseState, Value};

use crate::host::HostState;
use crate::registry::{SPEC_WEB_LOCKS_SETTLE, SPEC_WEB_LOCKS_REQUEST};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Exclusive,
    Shared,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Exclusive => "exclusive",
            Self::Shared => "shared",
        }
    }
}

struct Held {
    id: u64,
    mode: Mode,
}

struct LockQueue {
    held: Vec<Held>,
    pending: VecDeque<u64>,
}

struct Request {
    name: String,
    mode: Mode,
    callback: Value,
    promise: Rc<PromiseData>,
    steal: bool,
    granted: bool,
}

enum Completion {
    Fulfilled(Value),
    Rejected(Value),
}

pub struct LocksState {
    next_id: u64,
    queues: HashMap<String, LockQueue>,
    requests: HashMap<u64, Request>,
}

impl LocksState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            queues: HashMap::new(),
            requests: HashMap::new(),
        }
    }
}

pub fn build() -> Value {
    host_api::object(vec![(
        "request".into(),
        crate::host::capability(SPEC_WEB_LOCKS_REQUEST),
    )])
}

pub fn request(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let (options, callback) = match args.get(1) {
        Some(Value::Object(_) | Value::ObjectAlias(_)) if args.len() > 2 => (
            args.get(1).cloned().unwrap_or(Value::Undefined),
            args.get(2).cloned().unwrap_or(Value::Undefined),
        ),
        Some(value) => (Value::Undefined, value.clone()),
        None => (Value::Undefined, Value::Undefined),
    };
    if !quench_runtime::is_callable(&callback) {
        return Err(type_error("The callback argument must be a function"));
    }
    let mode = parse_mode(&options)?;
    let if_available = execute::is_truthy(&execute::get_property(&options, "ifAvailable"));
    let steal = execute::is_truthy(&execute::get_property(&options, "steal"));
    let promise = PromiseData::allocate(PromiseState::Pending);
    let id = {
        let mut host = state.borrow_mut();
        let id = host.locks.next_id;
        host.locks.next_id += 1;
        host.locks.requests.insert(
            id,
            Request {
                name: name.clone(),
                mode,
                callback,
                promise: Rc::clone(&promise),
                steal,
                granted: false,
            },
        );
        host.locks
            .queues
            .entry(name.clone())
            .or_insert_with(|| LockQueue {
                held: Vec::new(),
                pending: VecDeque::new(),
            })
            .pending
            .push_back(id);
        id
    };
    publish_request(state, id, "start", None)?;

    let busy = queue_busy(state, &name);
    if if_available && busy {
        remove_pending(state, &name, id);
        miss_request(state, id)?;
    } else if steal {
        abort_held(state, &name)?;
        remove_pending(state, &name, id);
        grant_request(state, id)?;
    } else if !busy {
        grant_request(state, id)?;
    }
    Ok(Value::Promise(promise))
}

fn parse_mode(options: &Value) -> Result<Mode, VmError> {
    let value = execute::get_property(options, "mode");
    if matches!(value, Value::Undefined) {
        return Ok(Mode::Exclusive);
    }
    match execute::to_js_string(&value)?.as_str() {
        "exclusive" => Ok(Mode::Exclusive),
        "shared" => Ok(Mode::Shared),
        _ => Err(type_error("The mode option must be either 'exclusive' or 'shared'")),
    }
}

fn queue_busy(state: &Rc<RefCell<HostState>>, name: &str) -> bool {
    state
        .borrow()
        .locks
        .queues
        .get(name)
        .is_some_and(|queue| !queue.held.is_empty())
}

fn remove_pending(state: &Rc<RefCell<HostState>>, name: &str, id: u64) {
    if let Some(queue) = state.borrow_mut().locks.queues.get_mut(name) {
        queue.pending.retain(|candidate| *candidate != id);
    }
}

fn grant_request(state: &Rc<RefCell<HostState>>, id: u64) -> Result<(), VmError> {
    let (name, mode, callback, steal) = {
        let mut host = state.borrow_mut();
        let request = host
            .locks
            .requests
            .get_mut(&id)
            .ok_or_else(|| type_error("Unknown lock request"))?;
        request.granted = true;
        let data = (
            request.name.clone(),
            request.mode,
            request.callback.clone(),
            request.steal,
        );
        let queue = host
            .locks
            .queues
            .get_mut(&data.0)
            .expect("request queue exists");
        queue.pending.retain(|candidate| *candidate != id);
        queue.held.push(Held { id, mode: data.1 });
        data
    };
    publish_request(state, id, "grant", None)?;
    let lock = host_api::object(vec![
        ("name".into(), Value::String(name)),
        ("mode".into(), Value::String(mode.as_str().into())),
    ]);
    let result = match execute::call(&callback, &Value::Undefined, &[lock]) {
        Ok(Value::Promise(promise)) => {
            let settle = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_WEB_LOCKS_SETTLE),
                vec![Value::Number(id as f64), Value::Boolean(false)],
            );
            let reject = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_WEB_LOCKS_SETTLE),
                vec![Value::Number(id as f64), Value::Boolean(true)],
            );
            quench_runtime::promise_then(Some(&Value::Promise(promise)), &[settle, reject])?;
            return Ok(());
        }
        Ok(value) => Completion::Fulfilled(value),
        Err(VmError::Suspended(promise)) => {
            let settle = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_WEB_LOCKS_SETTLE),
                vec![Value::Number(id as f64), Value::Boolean(false)],
            );
            let reject = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_WEB_LOCKS_SETTLE),
                vec![Value::Number(id as f64), Value::Boolean(true)],
            );
            quench_runtime::promise_then(Some(&Value::Promise(promise)), &[settle, reject])?;
            return Ok(());
        }
        Err(VmError::Thrown(error)) => Completion::Rejected(error),
        Err(_) => Completion::Rejected(Value::Undefined),
    };
    finish_request(state, id, result)
}

fn miss_request(state: &Rc<RefCell<HostState>>, id: u64) -> Result<(), VmError> {
    let callback = state
        .borrow()
        .locks
        .requests
        .get(&id)
        .map(|request| request.callback.clone())
        .ok_or_else(|| type_error("Unknown lock request"))?;
    publish_request(state, id, "miss", None)?;
    match execute::call(&callback, &Value::Undefined, &[Value::Null]) {
        Ok(Value::Promise(promise)) => {
            let settle = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_WEB_LOCKS_SETTLE),
                vec![Value::Number(id as f64), Value::Boolean(false)],
            );
            let reject = host_api::bound_capability_with_arguments(
                crate::host::capability_ref(SPEC_WEB_LOCKS_SETTLE),
                vec![Value::Number(id as f64), Value::Boolean(true)],
            );
            quench_runtime::promise_then(Some(&Value::Promise(promise)), &[settle, reject])?;
        }
        Ok(value) => finish_request(state, id, Completion::Fulfilled(value))?,
        Err(VmError::Thrown(error)) => finish_request(state, id, Completion::Rejected(error))?,
        Err(_) => finish_request(state, id, Completion::Rejected(Value::Undefined))?,
    }
    Ok(())
}

pub fn settle(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let id = match args.first() {
        Some(Value::Number(value)) => *value as u64,
        _ => return Ok(Value::Undefined),
    };
    let rejected = execute::is_truthy(&args.get(1).cloned().unwrap_or(Value::Boolean(false)));
    let value = args.get(2).cloned().unwrap_or(Value::Undefined);
    finish_request(
        state,
        id,
        if rejected {
            Completion::Rejected(value)
        } else {
            Completion::Fulfilled(value)
        },
    )?;
    Ok(Value::Undefined)
}

fn finish_request(
    state: &Rc<RefCell<HostState>>,
    id: u64,
    outcome: Completion,
) -> Result<(), VmError> {
    let Some(request) = state.borrow_mut().locks.requests.remove(&id) else {
        return Ok(());
    };
    let (error, rejected) = match outcome {
        Completion::Fulfilled(value) => (value, false),
        Completion::Rejected(error) => (error, true),
    };
    if request.granted {
        if let Some(queue) = state.borrow_mut().locks.queues.get_mut(&request.name) {
            queue.held.retain(|held| held.id != id);
        }
    }
    if rejected {
        quench_runtime::reject_promise(&request.promise, error.clone());
    } else {
        quench_runtime::resolve_promise(&request.promise, error.clone());
    }
    publish_request_data(
        state,
        &request.name,
        request.mode,
        request.steal,
        "end",
        rejected.then_some(error),
    )?;
    grant_next(state, &request.name)
}

fn abort_held(state: &Rc<RefCell<HostState>>, name: &str) -> Result<(), VmError> {
    let ids = state
        .borrow()
        .locks
        .queues
        .get(name)
        .map(|queue| queue.held.iter().map(|held| held.id).collect::<Vec<_>>())
        .unwrap_or_default();
    for id in ids {
        let Some(request) = state.borrow_mut().locks.requests.remove(&id) else {
            continue;
        };
        if let Some(queue) = state.borrow_mut().locks.queues.get_mut(name) {
            queue.held.retain(|held| held.id != id);
        }
        let error = abort_error();
        quench_runtime::reject_promise(&request.promise, error.clone());
        publish_request_data(
            state,
            &request.name,
            request.mode,
            request.steal,
            "end",
            Some(error),
        )?;
    }
    Ok(())
}

fn grant_next(state: &Rc<RefCell<HostState>>, name: &str) -> Result<(), VmError> {
    let next = {
        let host = state.borrow();
        let Some(queue) = host.locks.queues.get(name) else {
            return Ok(());
        };
        let Some(id) = queue.pending.front().copied() else {
            return Ok(());
        };
        let mode = host.locks.requests.get(&id).map(|request| request.mode);
        let compatible = queue.held.is_empty()
            || (mode == Some(Mode::Shared)
                && queue.held.iter().all(|held| held.mode == Mode::Shared));
        compatible.then_some(id)
    };
    if let Some(id) = next {
        grant_request(state, id)?;
        if state
            .borrow()
            .locks
            .queues
            .get(name)
            .is_some_and(|queue| queue.held.iter().all(|held| held.mode == Mode::Shared))
        {
            grant_next(state, name)?;
        }
    }
    Ok(())
}

fn publish_request(
    state: &Rc<RefCell<HostState>>,
    id: u64,
    event: &str,
    error: Option<Value>,
) -> Result<(), VmError> {
    let (name, mode, steal) = {
        let host = state.borrow();
        let Some(request) = host.locks.requests.get(&id) else {
            return Ok(());
        };
        (request.name.clone(), request.mode, request.steal)
    };
    publish_request_data(state, &name, mode, steal, event, error)
}

fn publish_request_data(
    state: &Rc<RefCell<HostState>>,
    name: &str,
    mode: Mode,
    steal: bool,
    event: &str,
    error: Option<Value>,
) -> Result<(), VmError> {
    let mut values = vec![
        ("name".into(), Value::String(name.into())),
        ("mode".into(), Value::String(mode.as_str().into())),
        ("steal".into(), Value::Boolean(steal)),
    ];
    if let Some(error) = error {
        values.push(("error".into(), error));
    }
    if event == "miss" {
        values.push(("ifAvailable".into(), Value::Boolean(true)));
    }
    crate::modules::diagnostics_channel::publish_named(
        state,
        &format!("locks.request.{event}"),
        host_api::object(values),
    )
}

fn abort_error() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("The operation was aborted".into())],
    );
    let _ = execute::set_property_in_place(&error, "name", Value::String("AbortError".into()));
    let _ = execute::set_property_in_place(&error, "code", Value::String("ABORT_ERR".into()));
    error
}

fn type_error(message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("message".into(), Value::String(message.into())),
    ]))
}
