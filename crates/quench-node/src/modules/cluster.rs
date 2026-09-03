//! Single-process `cluster` contract. Worker processes are deliberately absent.
use crate::host::HostState;
use crate::registry::{
    SPEC_CLUSTER_DISCONNECT, SPEC_CLUSTER_FORK, SPEC_CLUSTER_WORKER_DISCONNECT,
    SPEC_CLUSTER_WORKER_EMIT, SPEC_CLUSTER_WORKER_IS_CONNECTED, SPEC_CLUSTER_WORKER_IS_DEAD,
    SPEC_CLUSTER_WORKER_KILL, SPEC_CLUSTER_WORKER_ON, SPEC_CLUSTER_WORKER_PROCESS_SEND,
    SPEC_CLUSTER_WORKER_SEND,
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
    child_listeners: HashMap<String, Vec<Value>>,
    pending_messages: Vec<Value>,
    pending_disconnect: bool,
    pending_exit: Option<(i32, Option<String>)>,
    pending_worker_exit: Option<(Option<f64>, Option<String>)>,
}
pub struct ClusterState {
    next_id: u64,
    workers: HashMap<u64, Worker>,
    module: Option<Value>,
    worker_prototype: Option<Value>,
    script: Option<(String, String)>,
    pub(crate) worker_context: Option<u64>,
}
impl ClusterState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            workers: HashMap::new(),
            module: None,
            worker_prototype: None,
            script: None,
            worker_context: None,
        }
    }

    pub fn set_script(&mut self, filename: String, source: String) {
        self.script = Some((filename, source));
    }

    pub(crate) fn active_worker(&self) -> Option<Value> {
        self.worker_context
            .and_then(|id| self.workers.get(&id).map(|worker| worker.object.clone()))
    }

    pub(crate) fn worker_object(&self, id: u64) -> Option<Value> {
        self.workers.get(&id).map(|worker| worker.object.clone())
    }

    pub(crate) fn module(&self) -> Option<Value> {
        self.module.clone()
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
    let process = host_api::object(vec![
        (ID.into(), Value::Number(id as f64)),
        ("pid".into(), Value::Undefined),
        ("exitCode".into(), Value::Undefined),
        ("signalCode".into(), Value::Null),
        (
            "env".into(),
            args.first()
                .cloned()
                .unwrap_or_else(|| host_api::object(Vec::new())),
        ),
        ("connected".into(), Value::Boolean(true)),
        ("on".into(), crate::host::capability(SPEC_CLUSTER_WORKER_ON)),
        (
            "once".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_ON),
        ),
        (
            "emit".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
        ),
        (
            "disconnect".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_DISCONNECT),
        ),
        (
            "send".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_PROCESS_SEND),
        ),
        (
            "kill".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_KILL),
        ),
    ]);
    let mut worker = host_api::object(vec![
        (ID.into(), Value::Number(id as f64)),
        ("id".into(), Value::Number(id as f64)),
        ("process".into(), process.clone()),
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
            "once".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_ON),
        ),
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
        (
            "send".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_SEND),
        ),
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
            child_listeners: HashMap::new(),
            pending_messages: Vec::new(),
            pending_disconnect: false,
            pending_exit: None,
            pending_worker_exit: None,
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
    run_worker_script(state, id, &worker);
    Ok(worker)
}

fn run_worker_script(state: &Rc<RefCell<HostState>>, id: u64, worker: &Value) {
    let Some((filename, source)) = state.borrow().cluster.script.clone() else {
        return;
    };
    let Some(module) = state.borrow().cluster.module.clone() else {
        return;
    };
    for (key, value) in [
        ("isPrimary", Value::Boolean(false)),
        ("isMaster", Value::Boolean(false)),
        ("isWorker", Value::Boolean(true)),
        ("worker", worker.clone()),
    ] {
        let _ = execute::set_property_in_place(&module, key, value);
    }
    let _ = execute::set_property_in_place(worker, "state", Value::String("online".into()));
    let global = quench_runtime::vm::current_global_object();
    if let Ok(process) = execute::get_property_result(&global, "process") {
        let _ = execute::set_property_in_place(&process, ID, Value::Number(id as f64));
        let _ = execute::set_property_in_place(
            &process,
            "send",
            crate::host::capability(SPEC_CLUSTER_WORKER_PROCESS_SEND),
        );
    }
    let parent_exit_code = state.borrow().process.exit_code;
    state.borrow_mut().process.exit_code = None;
    state.borrow_mut().cluster.worker_context = Some(id);
    let wrapped = crate::modules::require::wrap_cjs(state, &filename, &source);
    let result =
        quench_runtime::reduce::reduce_global_script_source(&wrapped).and_then(|program| {
            let context = quench_runtime::vm::current_context();
            let mut registers = quench_runtime::register_file::RegisterFile::new();
            quench_runtime::vm::execute_code_in_place_context(
                program.code(),
                &mut registers,
                &context,
            )
            .map(|_| ())
            .map_err(|error| vec![error.render()])
        });
    // Child bootstrap and its first I/O notification run before `fork()`
    // returns, but under the child's module identity. Drain only the bounded
    // work made visible by this script; persistent work remains in the host
    // loop after the worker transitions back to primary mode.
    for _ in 0..64 {
        let _ = crate::modules::net::poll(state);
        match crate::modules::pump::drain_one_tick(state) {
            Ok(true) => {}
            Ok(false) | Err(_) => break,
        }
    }
    let child_exit_code = state.borrow().process.exit_code.or_else(|| {
        let net_work = crate::modules::net::has_work(state);
        let guard = state.borrow();
        let waits_for_ipc = guard
            .process
            .other_handlers
            .iter()
            .any(|(event, _, _)| event == "message")
            || guard.cluster.workers.get(&id).is_some_and(|worker| {
                worker.child_listeners.contains_key("message")
                    || !worker.pending_messages.is_empty()
            });
        (!waits_for_ipc && !net_work).then_some(0)
    });
    state.borrow_mut().process.exit_code = parent_exit_code;
    state.borrow_mut().cluster.worker_context = None;
    for (key, value) in [
        ("isPrimary", Value::Boolean(true)),
        ("isMaster", Value::Boolean(true)),
        ("isWorker", Value::Boolean(false)),
        ("worker", Value::Null),
    ] {
        let _ = execute::set_property_in_place(&module, key, value);
    }
    let global = quench_runtime::vm::current_global_object();
    if let Ok(process) = execute::get_property_result(&global, "process") {
        let _ = execute::delete_property(process.clone(), ID);
        let _ = execute::delete_property(process, "send");
    }
    if result.is_err() {
        state.borrow_mut().cluster.worker_context = None;
    }
    if let Some(code) = child_exit_code {
        close_worker_net(state, id);
        if let Some(worker_state) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker_state.connected = false;
            worker_state.dead = true;
            worker_state.pending_disconnect = true;
            worker_state.pending_exit = Some((code, None));
        }
        let _ =
            execute::set_property_in_place(worker, "state", Value::String("disconnected".into()));
        if let Ok(process) = execute::get_property_result(worker, "process") {
            let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
            let _ =
                execute::set_property_in_place(&process, "exitCode", Value::Number(code as f64));
            let _ = execute::set_property_in_place(&process, "signalCode", Value::Null);
        }
    }
}

pub(crate) fn set_worker_mode(
    state: &Rc<RefCell<HostState>>,
    id: u64,
    worker: &Value,
    child: bool,
) {
    if let Some(module) = state.borrow().cluster.module.clone() {
        for (key, value) in [
            ("isPrimary", Value::Boolean(!child)),
            ("isMaster", Value::Boolean(!child)),
            ("isWorker", Value::Boolean(child)),
            ("worker", if child { worker.clone() } else { Value::Null }),
        ] {
            let _ = execute::set_property_in_place(&module, key, value);
        }
    }
    let global = quench_runtime::vm::current_global_object();
    if let Ok(process) = execute::get_property_result(&global, "process") {
        if child {
            let _ = execute::set_property_in_place(&process, ID, Value::Number(id as f64));
        } else {
            let _ = execute::delete_property(process, ID);
        }
    }
}

fn close_worker_net(state: &Rc<RefCell<HostState>>, worker_id: u64) {
    let server_info = state
        .borrow()
        .net
        .servers
        .values()
        .filter_map(|server| {
            let server = server.borrow();
            server
                .owner_worker
                .filter(|owner| *owner == worker_id)
                .map(|_| (server.id, server.bind_addr))
        })
        .collect::<Vec<_>>();
    let server_ids = server_info
        .iter()
        .map(|(id, _)| *id)
        .collect::<std::collections::HashSet<_>>();
    let server_addrs = server_info
        .iter()
        .filter_map(|(_, address)| *address)
        .collect::<Vec<_>>();
    let servers = state
        .borrow()
        .net
        .servers
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for server in servers {
        let mut server = server.borrow_mut();
        if server.owner_worker == Some(worker_id) {
            server.listener.take();
            server.listening = false;
            server.closed = true;
        }
    }
    let sockets = state
        .borrow()
        .net
        .sockets
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for socket in sockets {
        let mut socket = socket.borrow_mut();
        if socket.server_id.is_some_and(|id| server_ids.contains(&id))
            || socket
                .local
                .is_some_and(|address| server_addrs.contains(&address))
            || socket
                .peer
                .is_some_and(|address| server_addrs.contains(&address))
        {
            socket.stream.take();
            socket.state = crate::modules::net::SocketState::Closed;
            socket.read_eof = true;
        }
    }
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
    let object = state
        .borrow()
        .cluster
        .workers
        .get(&id)
        .map(|worker| worker.object.clone())
        .unwrap_or_else(|| value.clone());
    Ok((id, object))
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
        let child = state.borrow().cluster.worker_context == Some(id);
        {
            let mut guard = state.borrow_mut();
            let listeners = if child {
                &mut guard.cluster.workers.get_mut(&id).unwrap().child_listeners
            } else {
                &mut guard.cluster.workers.get_mut(&id).unwrap().listeners
            };
            listeners.entry(name.clone()).or_default().push(cb.clone());
        }
        if name == "message" {
            let pending = state
                .borrow_mut()
                .cluster
                .workers
                .get_mut(&id)
                .map(|worker| std::mem::take(&mut worker.pending_messages))
                .unwrap_or_default();
            for message in pending {
                let _ = execute::call(cb, &obj, &[message]);
            }
        }
        if !child && name == "disconnect" {
            let pending = state
                .borrow()
                .cluster
                .workers
                .get(&id)
                .is_some_and(|worker| worker.pending_disconnect);
            if pending {
                state
                    .borrow_mut()
                    .cluster
                    .workers
                    .get_mut(&id)
                    .unwrap()
                    .pending_disconnect = false;
                let _ = execute::call(cb, &obj, &[]);
                if let Some(module) = state.borrow().cluster.module.clone() {
                    let _ = crate::modules::events::method_emit(
                        state,
                        Some(&module),
                        &[Value::String("disconnect".into()), obj.clone()],
                    );
                }
            }
        }
        if !child && name == "exit" {
            let pending = state
                .borrow_mut()
                .cluster
                .workers
                .get_mut(&id)
                .and_then(|worker| worker.pending_exit.take());
            if let Some((code, signal)) = pending {
                let _ = execute::call(
                    cb,
                    &obj,
                    &[
                        Value::Number(code as f64),
                        signal.clone().map(Value::String).unwrap_or(Value::Null),
                    ],
                );
                if let Some(module) = state.borrow().cluster.module.clone() {
                    let _ = crate::modules::events::method_emit(
                        state,
                        Some(&module),
                        &[
                            Value::String("exit".into()),
                            obj.clone(),
                            Value::Number(code as f64),
                            signal.clone().map(Value::String).unwrap_or(Value::Null),
                        ],
                    );
                }
            }
            let pending_worker = state
                .borrow_mut()
                .cluster
                .workers
                .get_mut(&id)
                .and_then(|worker| worker.pending_worker_exit.take());
            if let Some((code, signal)) = pending_worker {
                let _ = execute::call(
                    cb,
                    &obj,
                    &[
                        code.map(Value::Number).unwrap_or(Value::Null),
                        signal.map(Value::String).unwrap_or(Value::Null),
                    ],
                );
            }
        }
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
                let address = state
                    .borrow()
                    .net
                    .servers
                    .values()
                    .find_map(|server| {
                        let server = server.borrow();
                        (server.listening && server.bind_addr.is_some()).then(|| server.bind_addr)
                    })
                    .flatten();
                let info = host_api::object(vec![
                    (
                        "address".into(),
                        Value::String(
                            address
                                .map(|address| address.ip().to_string())
                                .unwrap_or_else(|| "127.0.0.1".into()),
                        ),
                    ),
                    (
                        "addressType".into(),
                        Value::Number(if address.is_some_and(|address| address.is_ipv6()) {
                            6.0
                        } else {
                            4.0
                        }),
                    ),
                    ("fd".into(), Value::Undefined),
                    (
                        "port".into(),
                        Value::Number(address.map_or(1, |address| address.port()) as f64),
                    ),
                ]);
                // A worker's listening notification is asynchronous. Queue
                // the callback so the parent can finish registering its
                // disconnect/exit listeners after `fork()` returns.
                state.borrow_mut().event_loop.queue_microtask_with_receiver(
                    cb.clone(),
                    vec![info],
                    obj.clone(),
                );
                let module = state.borrow().cluster.module.clone();
                if let Some(module) = module {
                    let _ = crate::modules::events::method_emit(
                        state,
                        Some(&module),
                        &[Value::String("listening".into()), obj.clone()],
                    );
                }
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
    let child = state.borrow().cluster.worker_context == Some(id);
    let callbacks = state
        .borrow()
        .cluster
        .workers
        .get(&id)
        .and_then(|w| {
            if child {
                w.child_listeners.get(&name).cloned()
            } else {
                w.listeners.get(&name).cloned()
            }
        })
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
        let _ = execute::set_property_in_place(&obj, "state", Value::String("disconnected".into()));
        if let Ok(process) = execute::get_property_result(&obj, "process") {
            let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
        }
    }
    let _ = emit(state, Some(&obj), &[Value::String("disconnect".into())]);
    if let Some(module) = state.borrow().cluster.module.clone() {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[Value::String("disconnect".into()), obj.clone()],
        );
    }
    let previous_context = state.borrow().cluster.worker_context;
    set_worker_mode(state, id, &obj, true);
    state.borrow_mut().cluster.worker_context = Some(id);
    let child_result = emit(state, Some(&obj), &[Value::String("disconnect".into())]);
    let child_exit = state.borrow().process.exit_code;
    state.borrow_mut().process.exit_code = None;
    state.borrow_mut().cluster.worker_context = previous_context;
    set_worker_mode(state, id, &obj, false);
    if child_result.is_err() || child_exit.is_some() {
        let code = child_exit.unwrap_or(1);
        close_worker_net(state, id);
        if let Some(w) = state.borrow_mut().cluster.workers.get_mut(&id) {
            w.dead = true;
        }
        let _ = emit(
            state,
            Some(&obj),
            &[
                Value::String("exit".into()),
                Value::Number(code as f64),
                Value::Null,
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
                    Value::Number(code as f64),
                    Value::Null,
                ],
            );
        }
    }
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
    let signal = args
        .first()
        .filter(|value| !quench_runtime::is_callable(value))
        .and_then(|value| execute::to_js_string(value).ok())
        .unwrap_or_else(|| "SIGTERM".into());
    close_worker_net(state, id);
    if let Some(w) = state.borrow_mut().cluster.workers.get_mut(&id) {
        w.connected = false;
        w.dead = true;
        if let Ok(process) = execute::get_property_result(&obj, "process") {
            let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
            let _ = execute::set_property_in_place(&process, "exitCode", Value::Null);
            let _ = execute::set_property_in_place(
                &process,
                "signalCode",
                Value::String(signal.clone()),
            );
        }
    }
    let _ = execute::set_property_in_place(&obj, "state", Value::String("disconnected".into()));
    let _ = emit(state, Some(&obj), &[Value::String("disconnect".into())]);
    if let Some(module) = state.borrow().cluster.module.clone() {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[Value::String("disconnect".into()), obj.clone()],
        );
    }
    let _ = execute::set_property_in_place(&obj, "state", Value::String("dead".into()));
    let worker_exit = emit(
        state,
        Some(&obj),
        &[
            Value::String("exit".into()),
            Value::Null,
            Value::String(signal.clone()),
        ],
    )
    .ok()
    .is_some_and(|value| execute::is_truthy(&value));
    if !worker_exit {
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.pending_worker_exit = Some((None, Some(signal.clone())));
        }
    }
    let module = state.borrow().cluster.module.clone();
    if let Some(module) = module {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[
                Value::String("exit".into()),
                obj.clone(),
                Value::Null,
                Value::String(signal),
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
    let child_context = state.borrow().cluster.worker_context == Some(id);
    if child_context {
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.pending_messages.push(message.clone());
        }
        return Ok(Value::Boolean(true));
    } else {
        let _ = emit(
            state,
            Some(&obj),
            &[Value::String("message".into()), message.clone()],
        );
    }
    if let Some(module) = state.borrow().cluster.module.clone() {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[
                Value::String("message".into()),
                obj.clone(),
                message.clone(),
            ],
        );
    }
    let previous_context = state.borrow().cluster.worker_context;
    set_worker_mode(state, id, &obj, true);
    state.borrow_mut().cluster.worker_context = Some(id);
    let process_result =
        crate::modules::process::emit(state, &[Value::String("message".into()), message]);
    state.borrow_mut().cluster.worker_context = previous_context;
    set_worker_mode(state, id, &obj, false);
    if process_result.is_err() {
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.connected = false;
            worker.dead = true;
        }
        let _ = emit(
            state,
            Some(&obj),
            &[
                Value::String("exit".into()),
                Value::Number(2.0),
                Value::Null,
            ],
        );
    }
    Ok(Value::Boolean(true))
}

/// Child-side `process.send(message)`: retain the message until the parent
/// attaches its worker listener after `fork()` returns.
pub fn process_send(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let id = worker(state, r).map(|(id, _)| id).or_else(|_| {
        state
            .borrow()
            .cluster
            .worker_context
            .ok_or_else(|| err("worker"))
    })?;
    let message = args.first().cloned().unwrap_or(Value::Undefined);
    let mut guard = state.borrow_mut();
    let active = guard.cluster.worker_context == Some(id);
    let Some(worker) = guard.cluster.workers.get_mut(&id) else {
        return Ok(Value::Boolean(false));
    };
    if !active || !worker.connected || worker.dead {
        return Ok(Value::Boolean(false));
    }
    worker.pending_messages.push(message);
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
