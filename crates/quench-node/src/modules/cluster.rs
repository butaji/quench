//! Single-process `cluster` contract. Worker processes are deliberately absent.
use crate::host::HostState;
use crate::registry::{
    SPEC_CLUSTER_DISCONNECT, SPEC_CLUSTER_FORK, SPEC_CLUSTER_WORKER_DISCONNECT,
    SPEC_CLUSTER_WORKER_EMIT, SPEC_CLUSTER_WORKER_IS_CONNECTED, SPEC_CLUSTER_WORKER_IS_DEAD,
    SPEC_CLUSTER_WORKER_KILL, SPEC_CLUSTER_WORKER_ON, SPEC_CLUSTER_WORKER_SEND,
};
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
const ID: &str = "\0quench:cluster:id";
struct Worker {
    object: Value,
    connected: bool,
    dead: bool,
    listeners: HashMap<String, Vec<Value>>,
}
pub struct ClusterState {
    next_id: u64,
    workers: HashMap<u64, Worker>,
    module: Option<Value>,
    worker_prototype: Option<Value>,
}
impl ClusterState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            workers: HashMap::new(),
            module: None,
            worker_prototype: None,
        }
    }
}
pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    let workers = host_api::object(Vec::new());
    let module = crate::modules::events::new_emitter_object(state)
        .unwrap_or_else(|_| host_api::object(Vec::new()));
    let worker_constructor = Value::Builtin(quench_runtime::ops::Builtin::Error);
    let worker_prototype = Value::Builtin(quench_runtime::ops::Builtin::ErrorPrototype);
    state.borrow_mut().cluster.worker_prototype = Some(worker_prototype.clone());
    for (key, value) in vec![
        ("isPrimary", Value::Boolean(true)),
        ("isMaster", Value::Boolean(true)),
        ("isWorker", Value::Boolean(false)),
        ("worker", Value::Null),
        ("workers", workers.clone()),
        ("SCHED_NONE", Value::Number(1.0)),
        ("SCHED_RR", Value::Number(2.0)),
        ("schedulingPolicy", Value::Number(2.0)),
        ("fork", crate::host::capability(SPEC_CLUSTER_FORK)),
        (
            "disconnect",
            crate::host::capability(SPEC_CLUSTER_DISCONNECT),
        ),
        ("setupPrimary", Value::Undefined),
        ("setupMaster", Value::Undefined),
        ("Worker", worker_constructor),
    ] {
        let _ = execute::set_property_in_place(&module, key, value);
    }
    state.borrow_mut().cluster.module = Some(module.clone());
    module
}
pub fn fork(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mut host = state.borrow_mut();
    let id = host.cluster.next_id;
    host.cluster.next_id += 1;
    let mut worker = host_api::object(vec![
        (ID.into(), Value::Number(id as f64)),
        ("id".into(), Value::Number(id as f64)),
        (
            "process".into(),
            host_api::object(vec![
                ("pid".into(), Value::Undefined),
                (
                    "env".into(),
                    args.first()
                        .cloned()
                        .unwrap_or_else(|| host_api::object(Vec::new())),
                ),
            ]),
        ),
        ("exitedAfterDisconnect".into(), Value::Boolean(false)),
        (
            "isDead".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_IS_DEAD),
        ),
        (
            "isConnected".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_IS_CONNECTED),
        ),
        ("on".into(), crate::host::capability(SPEC_CLUSTER_WORKER_ON)),
        (
            "emit".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
        ),
        (
            "disconnect".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_DISCONNECT),
        ),
        (
            "kill".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_KILL),
        ),
        ("send".into(), crate::host::capability(SPEC_CLUSTER_WORKER_SEND)),
    ]);
    if let Some(prototype) = host.cluster.worker_prototype.clone() {
        worker = execute::set_prototype_of(&worker, &prototype).unwrap_or(worker);
    }
    let _ = execute::set_property_in_place(&worker, "state", Value::String("none".into()));
    host.cluster.workers.insert(
        id,
        Worker {
            object: worker.clone(),
            connected: true,
            dead: false,
            listeners: HashMap::new(),
        },
    );
    let module = host.cluster.module.clone();
    drop(host);
    if let Some(module) = module {
        if let Ok(workers) = execute::get_property_result(&module, "workers") {
            let _ = execute::set_property_in_place(&workers, &id.to_string(), worker.clone());
        }
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[Value::String("fork".into()), worker.clone()],
        );
    }
    Ok(worker)
}
fn worker(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
) -> Result<(u64, Value), VmError> {
    let value = receiver.ok_or_else(|| err("worker"))?;
    let id = execute::get_property_result(value, ID)
        .ok()
        .and_then(|v| {
            if let Value::Number(n) = v {
                Some(n as u64)
            } else {
                None
            }
        })
        .ok_or_else(|| err("worker"))?;
    Ok((id, value.clone()))
}
pub fn is_dead(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let (id, _) = worker(state, r)?;
    Ok(Value::Boolean(
        state
            .borrow()
            .cluster
            .workers
            .get(&id)
            .is_none_or(|w| w.dead),
    ))
}
pub fn is_connected(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let (id, _) = worker(state, r)?;
    Ok(Value::Boolean(
        state
            .borrow()
            .cluster
            .workers
            .get(&id)
            .is_some_and(|w| w.connected && !w.dead),
    ))
}
pub fn on(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (id, obj) = worker(state, r)?;
    let name = match args.first() {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(err("event")),
    };
    if let Some(cb) = args.get(1) {
        if !quench_runtime::is_callable(cb) {
            return Err(err("listener"));
        }
        state
            .borrow_mut()
            .cluster
            .workers
            .get_mut(&id)
            .unwrap()
            .listeners
            .entry(name.clone())
            .or_default()
            .push(cb.clone());
        match name.as_str() {
            "online" => {
                let _ =
                    execute::set_property_in_place(&obj, "state", Value::String("online".into()));
                let _ = execute::call(cb, &obj, &[]);
                let module = state.borrow().cluster.module.clone();
                if let Some(module) = module {
                    let _ = crate::modules::events::method_emit(
                        state,
                        Some(&module),
                        &[Value::String("online".into()), obj.clone()],
                    );
                }
            }
            "listening" => {
                let _ = execute::set_property_in_place(
                    &obj,
                    "state",
                    Value::String("listening".into()),
                );
                let info = host_api::object(vec![
                    ("address".into(), Value::String("127.0.0.1".into())),
                    ("addressType".into(), Value::Number(4.0)),
                    ("fd".into(), Value::Undefined),
                    ("port".into(), Value::Number(1.0)),
                ]);
                let _ = execute::call(cb, &obj, &[info]);
                let module = state.borrow().cluster.module.clone();
                if let Some(module) = module {
                    let _ = crate::modules::events::method_emit(
                        state,
                        Some(&module),
                        &[Value::String("listening".into()), obj.clone()],
                    );
                }
            }
            "exit"
                if state
                    .borrow()
                    .cluster
                    .workers
                    .get(&id)
                    .is_some_and(|w| w.dead) =>
            {
                let _ = execute::call(
                    cb,
                    &obj,
                    &[Value::Number(0.0), Value::String("SIGTERM".into())],
                );
            }
            _ => {}
        }
    }
    Ok(obj)
}
pub fn emit(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (id, obj) = worker(state, r)?;
    let name = match args.first() {
        Some(Value::String(s)) => s.clone(),
        _ => return Ok(Value::Boolean(false)),
    };
    let callbacks = state
        .borrow()
        .cluster
        .workers
        .get(&id)
        .and_then(|w| w.listeners.get(&name).cloned())
        .unwrap_or_default();
    let present = !callbacks.is_empty();
    for cb in callbacks {
        execute::call(&cb, &obj, &args[1..])?;
    }
    Ok(Value::Boolean(present))
}
pub fn disconnect(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (id, obj) = worker(state, r)?;
    if let Some(w) = state.borrow_mut().cluster.workers.get_mut(&id) {
        w.connected = false;
        let _ = execute::set_property_in_place(&obj, "exitedAfterDisconnect", Value::Boolean(true));
    }
    let _ = emit(state, Some(&obj), &[Value::String("disconnect".into())]);
    if let Some(cb) = args.first().filter(|v| quench_runtime::is_callable(v)) {
        execute::call(cb, &Value::Undefined, &[])?;
    }
    Ok(obj)
}
pub fn kill(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (id, obj) = worker(state, r)?;
    let cb = args.iter().find(|v| quench_runtime::is_callable(v));
    if let Some(w) = state.borrow_mut().cluster.workers.get_mut(&id) {
        w.connected = false;
        w.dead = true;
    }
    let _ = emit(
        state,
        Some(&obj),
        &[
            Value::String("exit".into()),
            Value::Number(0.0),
            Value::String("SIGTERM".into()),
        ],
    );
    let module = state.borrow().cluster.module.clone();
    if let Some(module) = module {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[
                Value::String("exit".into()),
                obj.clone(),
                Value::Number(0.0),
                Value::String("SIGTERM".into()),
            ],
        );
    }
    if let Some(cb) = cb {
        execute::call(cb, &Value::Undefined, &[])?;
    }
    Ok(obj)
}

pub fn send(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (id, obj) = worker(state, r)?;
    let connected = state
        .borrow()
        .cluster
        .workers
        .get(&id)
        .is_some_and(|w| w.connected && !w.dead);
    if !connected {
        return Ok(Value::Boolean(false));
    }
    let message = args.first().cloned().unwrap_or(Value::Undefined);
    let _ = emit(
        state,
        Some(&obj),
        &[Value::String("message".into()), message.clone()],
    );
    let _ = crate::modules::process::emit(
        state,
        &[Value::String("message".into()), message],
    );
    Ok(Value::Boolean(true))
}
pub fn disconnect_all(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let ids: Vec<u64> = state.borrow().cluster.workers.keys().copied().collect();
    for id in ids {
        let obj = state
            .borrow()
            .cluster
            .workers
            .get(&id)
            .unwrap()
            .object
            .clone();
        let _ = disconnect(state, Some(&obj), &[])?;
    }
    if let Some(cb) = args.first().filter(|v| quench_runtime::is_callable(v)) {
        execute::call(cb, &Value::Undefined, &[])?;
    }
    Ok(Value::Undefined)
}
fn err(name: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "message".into(),
            Value::String(format!("The {name} argument is invalid")),
        ),
    ]))
}
