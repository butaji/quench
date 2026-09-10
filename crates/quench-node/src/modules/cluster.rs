//! Single-process `cluster` contract. Worker processes are deliberately absent.
use crate::host::HostState;
use crate::registry::{
    SPEC_CLUSTER_DISCONNECT, SPEC_CLUSTER_FORK, SPEC_CLUSTER_SETUP_EVENT,
    SPEC_CLUSTER_SETUP_MASTER, SPEC_CLUSTER_SETUP_PRIMARY, SPEC_CLUSTER_WORKER_CONSTRUCTOR,
    SPEC_CLUSTER_WORKER_DISCONNECT, SPEC_CLUSTER_WORKER_EMIT, SPEC_CLUSTER_WORKER_IS_CONNECTED,
    SPEC_CLUSTER_WORKER_IS_DEAD, SPEC_CLUSTER_WORKER_KILL, SPEC_CLUSTER_WORKER_ON,
    SPEC_CLUSTER_WORKER_PROCESS_SEND, SPEC_CLUSTER_WORKER_SEND, SPEC_EVENTS_EMIT,
};
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
const ID: &str = "\0quench:cluster:id";
// Process-event/timer scopes are separate from the transport scope used for
// listener sharing.  A cluster worker can therefore retain its own
// `process.on()` handlers without making its net handles look like a
// different primary process to the cluster scheduler.
pub(crate) const WORKER_EVENT_SCOPE_BASE: u64 = 1 << 48;
struct Worker {
    object: Value,
    /// Logical process scope; workers from separately forked primaries must
    /// not share a listener even though they inhabit one host VM.
    scope: u64,
    event_scope: u64,
    connected: bool,
    dead: bool,
    listeners: HashMap<String, Vec<Value>>,
    child_listeners: HashMap<String, Vec<Value>>,
    pending_messages: Vec<Value>,
    pending_listening: Vec<Value>,
    pending_disconnect: bool,
    pending_exit: Option<(i32, Option<String>)>,
    pending_worker_exit: Option<(Option<f64>, Option<String>)>,
    pending_child_disconnect: bool,
}
pub struct ClusterState {
    next_id: u64,
    workers: HashMap<u64, Worker>,
    module: Option<Value>,
    worker_prototype: Option<Value>,
    settings: Value,
    stdio: Option<Value>,
    script: Option<(String, String)>,
    pub(crate) worker_context: Option<u64>,
    process_scope: u64,
    fork_processes: HashMap<u64, Value>,
    worker_listen_slots: HashMap<u64, usize>,
    pending_cluster_listening: Vec<(Value, Value)>,
    /// Immutable parent argv snapshot used as the base for each logical
    /// worker re-entry. Worker bootstrap mutates the shared process view;
    /// deriving a later worker's argv from that view would duplicate prior
    /// worker arguments.
    parent_argv: Option<Value>,
    settings_explicit: bool,
    /// Primary-side IPC sender for each logical process scope. Worker event
    /// callbacks temporarily install their worker sender and restore this
    /// value when the callback returns.
    primary_sends: HashMap<u64, Value>,
}
impl ClusterState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            workers: HashMap::new(),
            module: None,
            worker_prototype: None,
            settings: host_api::object(Vec::new()),
            stdio: None,
            script: None,
            worker_context: None,
            process_scope: 0,
            fork_processes: HashMap::new(),
            worker_listen_slots: HashMap::new(),
            pending_cluster_listening: Vec::new(),
            parent_argv: None,
            settings_explicit: false,
            primary_sends: HashMap::new(),
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

    pub(crate) fn worker_scope(&self, id: u64) -> Option<u64> {
        self.workers.get(&id).map(|worker| worker.scope)
    }

    pub(crate) fn worker_event_scope(&self, id: u64) -> Option<u64> {
        self.workers.get(&id).map(|worker| worker.event_scope)
    }

    pub(crate) fn active_worker_event_scope(&self) -> Option<u64> {
        self.worker_context
            .and_then(|id| self.worker_event_scope(id))
    }

    pub(crate) fn worker_for_event_scope(&self, scope: u64) -> Option<u64> {
        self.workers
            .iter()
            .find_map(|(id, worker)| (worker.event_scope == scope).then_some(*id))
    }

    pub(crate) fn worker_parent_scope(&self, id: u64) -> Option<u64> {
        self.workers.get(&id).map(|worker| worker.scope)
    }

    pub(crate) fn worker_scopes(&self) -> HashMap<u64, u64> {
        self.workers
            .iter()
            .map(|(id, worker)| (*id, worker.scope))
            .collect()
    }

    pub(crate) fn process_scope(&self) -> u64 {
        self.process_scope
    }

    pub(crate) fn set_process_scope(&mut self, scope: u64) {
        self.process_scope = scope;
    }

    pub(crate) fn register_fork_process(&mut self, scope: u64, child: Value) {
        self.fork_processes.insert(scope, child);
    }

    pub(crate) fn take_fork_process(&mut self, scope: u64) -> Option<Value> {
        self.fork_processes.remove(&scope)
    }

    pub(crate) fn fork_process(&self, scope: u64) -> Option<Value> {
        self.fork_processes.get(&scope).cloned()
    }

    pub(crate) fn fork_scopes(&self) -> Vec<u64> {
        self.fork_processes.keys().copied().collect()
    }
}

/// Whether worker TCP servers should adopt a listener from the primary.
/// `SCHED_NONE` deliberately gives each worker its own bind attempt; the
/// resulting kernel error is observable by the worker's ordinary `error`
/// listener. Keep the policy as a fact on the public cluster object so JS
/// assignments remain the single source of truth without matching fixtures.
pub(crate) fn shares_listening_handle(state: &Rc<RefCell<HostState>>) -> bool {
    let global = quench_runtime::vm::current_global_object();
    let public = execute::get_property(&global, "__nodeCluster");
    let module = (!matches!(public, Value::Undefined | Value::Null))
        .then_some(public)
        .or_else(|| state.borrow().cluster.module.clone());
    module.is_none_or(|module| {
        !matches!(
            execute::get_property(&module, "schedulingPolicy"),
            Value::Number(policy) if policy == 1.0
        )
    })
}

/// Complete a child-process fork whose last referenced host handle has
/// closed.  IPC itself is not a reason to keep a Node child alive once its
/// event loop is idle; the parent observes the normal exit/close pair.
pub(crate) fn finish_idle_fork_process(
    state: &Rc<RefCell<HostState>>,
    scope: u64,
) -> Result<bool, VmError> {
    // A forked entry can synchronously re-enter cluster.fork().  That nested
    // worker bootstrap pumps the host while the parent scope is still active,
    // before the caller has had a chance to attach its IPC listeners.  Never
    // retire the scope currently executing; once fork() returns, the outer
    // poll observes the final listener/handle state normally.
    if state.borrow().cluster.process_scope() == scope {
        return Ok(false);
    }
    // `net::poll()` runs before nextTick/microtask draining. Keep a fork alive
    // whenever work already queued for that logical process can still settle
    // its exit status; otherwise a child that throws from nextTick is finalized
    // as exit(0) before the exception reaches the fork boundary.
    let pending_microtask = state
        .borrow()
        .event_loop
        .microtasks
        .borrow()
        .iter()
        .any(|task| task.process_scope == scope);
    if pending_microtask {
        return Ok(false);
    }
    if crate::modules::net::has_live_scope(state, scope)
        || crate::modules::process::has_listener_in_scope(state, "message", scope)
    {
        return Ok(false);
    }
    let child = state.borrow().cluster.fork_process(scope);
    let Some(child) = child else {
        return Ok(false);
    };
    // Terminal transitions such as AbortSignal and kill() publish their
    // exit/close pair directly.  The fork remains registered until the
    // event-loop poll reaches this lifecycle boundary, so make completion
    // idempotent instead of manufacturing a second exit(0) pair.
    if matches!(
        execute::get_property(&child, "\0childTerminated"),
        Value::Boolean(true)
    ) {
        state.borrow_mut().cluster.take_fork_process(scope);
        return Ok(false);
    }
    // A fork can fail during invocation validation before its queued spawn
    // phase runs. Keep the logical child registered until that phase emits
    // the terminal stderr/exit events after fork() returns and listeners can
    // observe them.
    if matches!(
        execute::get_property(&child, "\0forkStartupFailed"),
        Value::Boolean(true)
    ) {
        return Ok(false);
    }
    let timer_live = match execute::get_property(&child, "\0childTimerIds") {
        Value::Array(ids) => (0..ids.logical_len()).any(|index| {
            matches!(
                execute::get_property_result(&Value::Array(ids.clone()), &index.to_string()),
                Ok(Value::Number(id)) if id.is_finite() && state.borrow().timers.timers.contains_key(&(id as u64))
            )
        }),
        _ => false,
    };
    if timer_live {
        return Ok(false);
    }
    let child = state.borrow_mut().cluster.take_fork_process(scope);
    let Some(child) = child else {
        return Ok(false);
    };
    execute::set_property_in_place(&child, "connected", Value::Boolean(false));
    execute::set_property_in_place(&child, "exitCode", Value::Number(0.0));
    execute::set_property_in_place(&child, "signalCode", Value::Null);
    if let Value::Number(pid) = execute::get_property(&child, "pid") {
        state.borrow_mut().process.alive_pids.remove(&(pid as i64));
    }
    let previous_scope = state.borrow().cluster.process_scope();
    let previous_event_scope = state.borrow().event_loop.process_scope();
    state.borrow_mut().cluster.set_process_scope(0);
    state.borrow().event_loop.set_process_scope(0);
    for event in ["exit", "close"] {
        crate::modules::events::method_emit(
            state,
            Some(&child),
            &[
                Value::String(event.into()),
                Value::Number(0.0),
                Value::Null,
            ],
        )?;
    }
    state.borrow_mut().cluster.set_process_scope(previous_scope);
    state
        .borrow()
        .event_loop
        .set_process_scope(previous_event_scope);
    state.borrow_mut().emitters.remove_scope(scope);
    Ok(true)
}

pub fn setup_primary(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let supplied = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    let merged = merge_settings(state, &supplied);
    let stdio = execute::get_property(&merged, "stdio");
    let module = state.borrow().cluster.module.clone();
    {
        let mut host = state.borrow_mut();
        host.cluster.settings = merged.clone();
        host.cluster.stdio = Some(stdio);
        host.cluster.settings_explicit = true;
    }
    if let Some(module) = module {
        execute::set_property_in_place(&module, "settings", merged);
        state.borrow().event_loop.queue_microtask_with_receiver(
            crate::host::capability(SPEC_CLUSTER_SETUP_EVENT),
            Vec::new(),
            module,
        );
    }
    Ok(Value::Undefined)
}

fn merge_settings(state: &Rc<RefCell<HostState>>, supplied: &Value) -> Value {
    let current = state.borrow().cluster.settings.clone();
    let mut values = Vec::new();
    for key in ["args", "exec", "execArgv", "silent", "stdio"] {
        let value = match execute::get_property(supplied, key) {
            Value::Undefined => execute::get_property(&current, key),
            value => value,
        };
        if !matches!(value, Value::Undefined) {
            values.push((key.to_string(), value));
        }
    }
    if values.iter().all(|(key, _)| key != "args") {
        let process = quench_runtime::vm::current_global_object();
        if let Ok(process) = execute::get_property_result(&process, "process") {
            values.push(("args".into(), process_args(&process)));
        }
    }
    if values.iter().all(|(key, _)| key != "exec") {
        let process = quench_runtime::vm::current_global_object();
        let exec = execute::get_property_result(&process, "process")
            .map(|process| execute::get_property(&process, "argv"))
            .map(|argv| execute::get_property(&argv, "1"))
            .unwrap_or(Value::Undefined);
        if !matches!(exec, Value::Undefined) {
            values.push(("exec".into(), exec));
        }
    }
    if values.iter().all(|(key, _)| key != "execArgv") {
        values.push(("execArgv".into(), host_api::array(Vec::new())));
    }
    if values.iter().all(|(key, _)| key != "silent") {
        values.push(("silent".into(), Value::Boolean(false)));
    }
    for key in execute::own_enumerable_keys(supplied) {
        if !["args", "exec", "execArgv", "silent", "stdio"].contains(&key.as_str()) {
            values.push((key.clone(), execute::get_property(supplied, &key)));
        }
    }
    host_api::object(values)
}

fn process_args(process: &Value) -> Value {
    let argv = execute::get_property(process, "argv");
    let length = match execute::get_property(&argv, "length") {
        Value::Number(length) if length >= 2.0 => length as usize,
        _ => 0,
    };
    host_api::array(
        (2..length)
            .map(|index| execute::get_property(&argv, &index.to_string()))
            .collect(),
    )
}

fn array_values(array: &Value) -> Vec<Value> {
    let Value::Array(values) = array else {
        return Vec::new();
    };
    let length = match execute::get_property(array, "length") {
        Value::Number(length) if length >= 0.0 => length as usize,
        _ => values.logical_len(),
    };
    (0..length)
        .map(|index| execute::get_property(array, &index.to_string()))
        .collect()
}

pub fn setup_event(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        let _ = crate::modules::events::method_emit(
            state,
            Some(receiver),
            &[Value::String("setup".into())],
        );
    }
    Ok(Value::Undefined)
}

fn worker_value(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Value {
    let options = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    let id = match execute::get_property(&options, "id") {
        Value::Number(value) => Value::Number(value),
        _ => Value::Number(0.0),
    };
    let worker_state = match execute::get_property(&options, "state") {
        Value::String(value) => Value::String(value),
        _ => Value::String("none".into()),
    };
    let mut worker = host_api::object(vec![
        (ID.into(), id.clone()),
        ("id".into(), id),
        ("state".into(), worker_state),
        ("process".into(), execute::get_property(&options, "process")),
        ("exitedAfterDisconnect".into(), Value::Undefined),
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
    if let Some(prototype) = state.borrow().cluster.worker_prototype.clone() {
        worker = execute::set_prototype_of(&worker, &prototype).unwrap_or(worker);
    }
    worker
}

pub fn worker_construct(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(worker_value(state, args))
}

pub fn worker_construct_handler(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(worker_value(state, args))
}

fn stdio_channel(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    let configured = state.borrow().cluster.stdio.clone()?;
    let Value::Array(slots) = configured else {
        return None;
    };
    let pipe = execute::get_property(&Value::Array(slots), "4");
    if !matches!(pipe, Value::String(value) if value == "pipe") {
        return None;
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let address = listener.local_addr().ok()?;
    let parent = TcpStream::connect(address).ok()?;
    let (child, _) = listener.accept().ok()?;
    let _ = parent.set_nonblocking(true);
    let _ = child.set_nonblocking(true);
    crate::modules::net::register_fd_stream(state, 4, child);
    crate::modules::net::register_fd_stream(state, 4, parent);
    let options = host_api::object(vec![
        ("fd".into(), Value::Number(4.0)),
        ("allowHalfOpen".into(), Value::Boolean(false)),
    ]);
    crate::modules::net::socket_construct(state, &[options]).ok()
}

fn enter_worker_env(process: &Value, worker: &Value) -> Vec<(String, Value)> {
    let env = execute::get_property(process, "env");
    let worker_env = execute::get_property(&execute::get_property(worker, "process"), "env");
    let mut previous = Vec::new();
    for key in execute::own_enumerable_keys(&worker_env) {
        previous.push((key.clone(), execute::get_property(&env, &key)));
        execute::set_property_in_place(&env, &key, execute::get_property(&worker_env, &key));
    }
    previous
}

fn restore_worker_env(process: &Value, previous: Vec<(String, Value)>) {
    let env = execute::get_property(process, "env");
    for (key, value) in previous {
        if matches!(value, Value::Undefined) {
            let _ = execute::delete_property(env.clone(), &key);
        } else {
            execute::set_property_in_place(&env, &key, value);
        }
    }
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    let workers = host_api::object(Vec::new());
    let module = crate::modules::events::new_emitter_object(state)
        .unwrap_or_else(|_| host_api::object(Vec::new()));
    let worker_prototype = host_api::object(Vec::new());
    let worker_constructor = execute::set_property(
        crate::host::capability(SPEC_CLUSTER_WORKER_CONSTRUCTOR),
        "prototype",
        worker_prototype.clone(),
    );
    if state.borrow().cluster.parent_argv.is_none() {
        let global = quench_runtime::vm::current_global_object();
        let argv = execute::get_property(&execute::get_property(&global, "process"), "argv");
        state.borrow_mut().cluster.parent_argv = Some(argv);
    }
    state.borrow_mut().cluster.worker_prototype = Some(worker_prototype.clone());
    for (key, value) in vec![
        ("isPrimary", Value::Boolean(true)),
        ("isMaster", Value::Boolean(true)),
        ("isWorker", Value::Boolean(false)),
        ("worker", Value::Null),
        ("settings", state.borrow().cluster.settings.clone()),
        ("workers", workers.clone()),
        ("SCHED_NONE", Value::Number(1.0)),
        ("SCHED_RR", Value::Number(2.0)),
        ("schedulingPolicy", Value::Number(2.0)),
        ("fork", crate::host::capability(SPEC_CLUSTER_FORK)),
        (
            "disconnect",
            crate::host::capability(SPEC_CLUSTER_DISCONNECT),
        ),
        // The single-process host has no child-process launch settings, but
        // both setup entry points remain callable so API consumers can use
        // the same lifecycle before `fork()`.
        (
            "setupPrimary",
            crate::host::capability(SPEC_CLUSTER_SETUP_PRIMARY),
        ),
        (
            "setupMaster",
            crate::host::capability(SPEC_CLUSTER_SETUP_MASTER),
        ),
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
    let channel = stdio_channel(state);
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
        ("\0clusterProcessSender".into(), Value::Boolean(true)),
        (
            "stdio".into(),
            host_api::array(
                (0..5)
                    .map(|index| {
                        if index == 4 {
                            channel.clone().unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        }
                    })
                    .collect(),
            ),
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
            "send".into(),
            crate::host::capability(SPEC_CLUSTER_WORKER_PROCESS_SEND),
        ),
        // Cluster workers expose their IPC channel to worker bootstrap code.
        // This host keeps the transport in-process, but the handle still
        // needs the ordinary callable ref/unref surface.
        (
            "channel".into(),
            host_api::object(vec![
                (
                    "ref".into(),
                    crate::host::capability(crate::registry::SPEC_PROCESS_REF),
                ),
                (
                    "unref".into(),
                    crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
                ),
            ]),
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
    let process_scope = host.cluster.process_scope;
    host.cluster.workers.insert(
        id,
        Worker {
            object: worker.clone(),
            scope: process_scope,
            event_scope: WORKER_EVENT_SCOPE_BASE + id,
            connected: true,
            dead: false,
            listeners: HashMap::new(),
            child_listeners: HashMap::new(),
            pending_messages: Vec::new(),
            pending_listening: Vec::new(),
            pending_disconnect: false,
            pending_exit: None,
            pending_worker_exit: None,
            pending_child_disconnect: false,
        },
    );
    let module = host.cluster.module.clone();
    drop(host);
    let primary_process = {
        let global = quench_runtime::vm::current_global_object();
        execute::get_property(&global, "process")
    };
    let primary_send = execute::get_property(&primary_process, "send");
    let primary_scope = state.borrow().cluster.process_scope();
    state
        .borrow_mut()
        .cluster
        .primary_sends
        .entry(primary_scope)
        .or_insert_with(|| primary_send.clone());
    if let Some(module) = module {
        if let Ok(workers) = execute::get_property_result(&module, "workers") {
            let _ = execute::set_property_in_place(&workers, &id.to_string(), worker.clone());
        }
    }
    run_worker_script(state, id, &worker);
    // Worker re-entry temporarily installs the cluster worker's process.send
    // capability. Restore the primary IPC sender before fork() returns so a
    // listener registered immediately after the call binds the parent
    // channel rather than the worker-side sender.
    execute::set_property_in_place(&primary_process, "send", primary_send);
    // Child bootstrap may have delivered a listening notification while the
    // logical worker was re-entered. The parent-visible lifecycle still
    // starts at `none` for the deferred fork event; the queued listening
    // notification advances it after fork/online observers run.
    let _ = execute::set_property_in_place(&worker, "state", Value::String("none".into()));
    // The worker bootstrap above drains the current microtask queue. Queue
    // `fork` only after that re-entry so it cannot run before `cluster.fork()`
    // returns its Worker object to the caller.
    if let Some(module) = state.borrow().cluster.module.clone() {
        state.borrow().event_loop.queue_microtask_with_receiver(
            crate::host::capability(SPEC_EVENTS_EMIT),
            vec![Value::String("fork".into()), worker.clone()],
            module,
        );
    }
    // The online transition follows `fork` and owns the state change. This
    // preserves the observable `none` state during the fork notification and
    // gives listeners attached to the returned Worker the same next-turn
    // online event as Node.
    state.borrow().event_loop.queue_microtask_with_receiver(
        crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
        vec![Value::String("online".into())],
        worker.clone(),
    );
    let pending_listening = state
        .borrow_mut()
        .cluster
        .pending_cluster_listening
        .drain(..)
        .collect::<Vec<_>>();
    let module = { state.borrow().cluster.module.clone() };
    if let Some(module) = module {
        let process_scope = state.borrow().cluster.process_scope();
        for (worker, address) in pending_listening {
            state
                .borrow()
                .event_loop
                .queue_microtask_with_receiver_scope(
                    crate::host::capability(SPEC_EVENTS_EMIT),
                    vec![Value::String("listening".into()), worker, address],
                    module.clone(),
                    process_scope,
                );
        }
    }
    Ok(worker)
}

fn run_worker_script(state: &Rc<RefCell<HostState>>, id: u64, worker: &Value) {
    let Some((filename, source)) = state.borrow().cluster.script.clone() else {
        return;
    };
    let Some(module) = state.borrow().cluster.module.clone() else {
        return;
    };
    // A cluster worker is a separate Node process. Keep the primary's global
    // object as the restoration target while the worker bootstrap runs, so
    // worker-only globals and polyfill bindings cannot leak back into the
    // parent after re-entry.
    let parent_global = quench_runtime::vm::current_global_object();
    let parent_global_entries = match &parent_global {
        Value::Object(object) => object
            .iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    for (key, value) in [
        ("isPrimary", Value::Boolean(false)),
        ("isMaster", Value::Boolean(false)),
        ("isWorker", Value::Boolean(true)),
        ("worker", worker.clone()),
    ] {
        let _ = execute::set_property_in_place(&module, key, value);
    }
    let global = quench_runtime::vm::current_global_object();
    let previous_quench_argv = execute::get_property(&global, "__quench_argv");
    let previous_quench_argv_values = array_values(&previous_quench_argv);
    let process_value = execute::get_property_result(&global, "process").ok();
    let previous_process_id = process_value
        .as_ref()
        .map(|process| execute::get_property(process, ID));
    let previous_process_send = process_value
        .as_ref()
        .map(|process| execute::get_property(process, "send"));
    let previous_process_disconnect = process_value
        .as_ref()
        .map(|process| execute::get_property(process, "disconnect"));
    let previous_process_channel = process_value
        .as_ref()
        .map(|process| execute::get_property(process, "channel"));
    let previous_process_connected = process_value
        .as_ref()
        .map(|process| execute::get_property(process, "connected"));
    let env_restore = process_value
        .as_ref()
        .map(|process| enter_worker_env(process, worker));
    let previous_argv = state
        .borrow()
        .cluster
        .parent_argv
        .clone()
        .or_else(|| {
            process_value
                .as_ref()
                .map(|process| execute::get_property(process, "argv"))
        });
    let worker_args = if state.borrow().cluster.settings_explicit {
        state.borrow().cluster.settings.clone()
    } else {
        process_value
            .as_ref()
            .map(process_args)
            .unwrap_or_else(|| host_api::array(Vec::new()))
    };
    if let (Some(process), Some(previous_argv)) = (&process_value, &previous_argv) {
        let args = if state.borrow().cluster.settings_explicit {
            execute::get_property(&worker_args, "args")
        } else {
            worker_args.clone()
        };
        let mut values = array_values(previous_argv);
        if let Value::Array(args) = args {
            values.extend(array_values(&Value::Array(args)));
        }
        let worker_argv = host_api::array(values.clone());
        execute::set_property_in_place(process, "argv", worker_argv.clone());
        // The globals bootstrap reconstructs `process.argv` from this host
        // fact on every logical worker re-entry. Keep that derived surface in
        // sync with the worker view so nested cluster workers receive the
        // same command-line arguments instead of re-entering as primaries.
        let qargv_target = execute::get_property(&global, "__quench_argv");
        if matches!(qargv_target, Value::Array(_)) {
            execute::set_array_length_in_place(&qargv_target, 0);
            for (index, value) in values.iter().cloned().enumerate() {
                execute::set_array_element_in_place(&qargv_target, index, value);
            }
        } else {
            let _ = execute::set_property_in_place(&global, "__quench_argv", worker_argv.clone());
        }
    }
    if let Ok(process) = execute::get_property_result(&global, "process") {
        let _ = execute::set_property_in_place(&process, ID, Value::Number(id as f64));
        let disconnect = quench_runtime::host_api::bound_capability_with_arguments(
            crate::host::capability_ref(SPEC_CLUSTER_WORKER_DISCONNECT),
            vec![worker.clone()],
        );
        let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(true));
        let _ = execute::set_property_in_place(&process, "disconnect", disconnect);
        let _ = execute::set_property_in_place(
            &process,
            "send",
            crate::host::capability(SPEC_CLUSTER_WORKER_PROCESS_SEND),
        );
        let worker_process = execute::get_property(worker, "process");
        let worker_channel = execute::get_property(&worker_process, "channel");
        let _ = execute::set_property_in_place(&process, "channel", worker_channel);
        let _ = execute::set_property_in_place(
            &process,
            "\0clusterProcessSender",
            Value::Boolean(true),
        );
    }
    let parent_exit_code = state.borrow().process.exit_code;
    state.borrow_mut().process.exit_code = None;
    state.borrow_mut().cluster.worker_context = Some(id);
    state.borrow_mut().cluster.worker_listen_slots.insert(id, 0);
    let wrapped = crate::modules::require::wrap_cjs(state, &filename, &source);
    // A cluster worker re-enters the VM after the runner bootstrap has
    // completed. Reinstall the runner-owned web/global surfaces before
    // evaluating its CommonJS entry: shared helpers (notably common.js)
    // legitimately inspect the standard fetch binding during startup.
    let web_streams_surface = crate::polyfills::bootstrap::lookup("web-streams").unwrap_or("");
    let globals_surface = crate::polyfills::bootstrap::lookup("globals-extra").unwrap_or("");
    let fetch_surface = crate::polyfills::bootstrap::lookup("fetch").unwrap_or("");
    let worker_bootstrap =
        format!("{web_streams_surface}\n{globals_surface}\n{fetch_surface}\nconst fetch = globalThis.fetch;");
    // Each logical worker re-entry gets a fresh lexical scope. The shared VM
    // global is restored after execution, but top-level `const`/`let`
    // bindings live in the realm's lexical environment and would otherwise
    // collide when the next worker bootstraps the same helpers.
    let wrapped = format!("{{\n{worker_bootstrap}\n{wrapped}\n}}");
    let result =
        quench_runtime::reduce::reduce_global_script_source(&wrapped).and_then(|program| {
            // Cluster workers re-enter this VM synchronously. Bound only this
            // logical process so an uncooperative worker yields to the
            // primary state machine and can be terminated through its public
            // process handle.
            let context = (*quench_runtime::vm::current_context())
                .clone()
                .with_execution_budget(100_000);
            quench_runtime::vm::execute_code_isolated_in_context(program.code(), &context)
                .map(|_| ())
                .map_err(|error| vec![error.render()])
        });
    // Promise-backed fallbacks used by worker bootstrap APIs (for example
    // dgram's setImmediate compatibility path) must settle while the worker
    // process view is still installed. Otherwise their callbacks observe the
    // primary's `cluster.worker === null` after this re-entry returns.
    for _ in 0..8 {
        if !quench_runtime::has_pending_promise_jobs() { break; }
        quench_runtime::drain_promise_jobs();
    }
    // Child bootstrap and its first I/O notification run before `fork()`
    // returns, but under the child's module identity. Drain only the bounded
    // work made visible by this script; persistent work remains in the host
    // loop after the worker transitions back to primary mode.
    for _ in 0..256 {
        if let Err(error) = crate::modules::pump::drain_immediates(state) {
            if crate::modules::pump::handle_uncaught(state, error).is_ok() {
                let _ = crate::modules::pump::run_uncaught(state);
            } else {
                break;
            }
        }
        if let Err(error) = crate::modules::net::poll(state) {
            if crate::modules::pump::handle_uncaught(state, error).is_ok() {
                let _ = crate::modules::pump::run_uncaught(state);
            } else {
                break;
            }
        }
        match crate::modules::pump::drain_one_tick(state) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => break,
        }
    }
    let worker_global = quench_runtime::vm::current_global_object();
    if worker_global.object_identity() != parent_global.object_identity() {
        quench_runtime::execute::replace_global_object(&worker_global, &parent_global);
    }
    // Worker bootstrap and its required modules may have added or replaced
    // properties on the shared host object. Restore the complete parent
    // property projection so worker-only globals cannot escape into primary;
    // this is a generic process-boundary rule, not a list of fixture names.
    let mut restored_global = parent_global.clone();
    if let Value::Object(global) = &restored_global {
        let original_names = parent_global_entries
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let current_names = global
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>();
        for name in current_names {
            if !original_names.contains(name.as_str()) {
                let (updated, deleted) =
                    quench_runtime::execute::delete_property(restored_global.clone(), &name);
                if deleted {
                    quench_runtime::execute::replace_global_object(&restored_global, &updated);
                    restored_global = updated;
                }
            }
        }
        for (name, value) in &parent_global_entries {
            let _ = quench_runtime::execute::set_property_in_place(
                &restored_global,
                name,
                value.clone(),
            );
        }
    }
    let child_exit_code = state.borrow().process.exit_code.or_else(|| {
        let net_work = crate::modules::net::has_work(state);
        let worker_has_message_listener = crate::modules::process::has_listener_in_scope(
            state,
            "message",
            WORKER_EVENT_SCOPE_BASE + id,
        );
        let guard = state.borrow();
        let waits_for_ipc = guard
            .process
            .other_handlers
            .iter()
            .any(|(event, _, _)| event == "message")
            || worker_has_message_listener
            || guard.cluster.workers.get(&id).is_some_and(|worker| {
                worker.child_listeners.contains_key("message")
                    || !worker.pending_messages.is_empty()
            });
        (!waits_for_ipc && !net_work).then_some(0)
    });
    state.borrow_mut().process.exit_code = parent_exit_code;
    if let (Some(process), Some(previous)) = (&process_value, env_restore) {
        restore_worker_env(process, previous);
    }
    state.borrow_mut().cluster.worker_context = None;
    for (key, value) in [
        ("isPrimary", Value::Boolean(true)),
        ("isMaster", Value::Boolean(true)),
        ("isWorker", Value::Boolean(false)),
        ("worker", Value::Null),
    ] {
        let _ = execute::set_property_in_place(&module, key, value);
    }
    let global = restored_global;
    if matches!(previous_quench_argv, Value::Array(_)) {
        execute::set_array_length_in_place(&previous_quench_argv, 0);
        for (index, value) in previous_quench_argv_values.into_iter().enumerate() {
            execute::set_array_element_in_place(&previous_quench_argv, index, value);
        }
    } else {
        let _ = execute::set_property_in_place(&global, "__quench_argv", previous_quench_argv);
    }
    if let Ok(process) = execute::get_property_result(&global, "process") {
        if let Some(previous_argv) = previous_argv {
            let _ = execute::set_property_in_place(&process, "argv", previous_argv);
        }
        for (key, value) in [
            (ID, previous_process_id.unwrap_or(Value::Undefined)),
            ("send", previous_process_send.unwrap_or(Value::Undefined)),
            (
                "disconnect",
                previous_process_disconnect.unwrap_or(Value::Undefined),
            ),
            (
                "connected",
                previous_process_connected.unwrap_or(Value::Undefined),
            ),
            ("channel", previous_process_channel.unwrap_or(Value::Undefined)),
        ] {
            if matches!(value, Value::Undefined) {
                let _ = execute::delete_property(process.clone(), key);
            } else {
                let _ = execute::set_property_in_place(&process, key, value);
            }
        }
        let _ = execute::delete_property(process, "\0clusterProcessSender");
    }
    // Worker re-entry must not turn runner bookkeeping into enumerable
    // process globals. The upstream leak check observes the global object
    // after worker teardown, so restore the hidden descriptors explicitly.
    for key in ["__nodeCurrentAsyncResource", "__nodeCallChecks"] {
        let value = execute::get_property(&global, key);
        if !matches!(value, Value::Undefined) {
            let descriptor = host_api::object(vec![
                ("value".into(), value),
                ("writable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(false)),
            ]);
            let _ = execute::define_property(global.clone(), key, descriptor);
        }
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
            // Worker callbacks execute in the shared VM but must observe the
            // worker-side IPC sender while their logical process scope is
            // active.  This is the same capability installed during worker
            // bootstrap; keeping it scoped here also covers delayed I/O
            // callbacks that run after `fork()` has returned.
            let _ = execute::set_property_in_place(
                &process,
                "send",
                crate::host::capability(SPEC_CLUSTER_WORKER_PROCESS_SEND),
            );
        } else {
            let scope = state.borrow().cluster.process_scope();
            let primary_send = state
                .borrow()
                .cluster
                .primary_sends
                .get(&scope)
                .cloned()
                .unwrap_or(Value::Undefined);
            let _ = execute::delete_property(process.clone(), ID);
            // Restore the sender belonging to this logical primary.  The
            // top-level primary intentionally has no `process.send`, while a
            // child-process primary retains its ordinary IPC capability.
            let _ = execute::set_property_in_place(&process, "send", primary_send);
        }
    }
}

/// Queue a primary cluster `listening` notification for a worker-owned server.
pub(crate) fn notify_listening(state: &Rc<RefCell<HostState>>, worker_id: u64, address: Value) {
    if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&worker_id) {
        worker.pending_listening.push(address.clone());
    }
    // `fork().on(...)` registers after synchronous worker re-entry. Retain a
    // module-level event until `fork` has returned and can queue it after the
    // caller installs listeners on `cluster`.
    let Some(worker) = state.borrow().cluster.worker_object(worker_id) else {
        return;
    };
    state
        .borrow_mut()
        .cluster
        .pending_cluster_listening
        .push((worker, listening_info(&address)));
}

/// Reserve the next construction-order slot for a worker's ephemeral server.
/// Cluster's `listen(0)` shares one descriptor per logical creation slot
/// across workers; the slot is reset for each worker script re-entry.
pub(crate) fn next_worker_listen_slot(state: &Rc<RefCell<HostState>>) -> Option<usize> {
    let worker_id = state.borrow().cluster.worker_context?;
    let mut guard = state.borrow_mut();
    let slot = guard
        .cluster
        .worker_listen_slots
        .entry(worker_id)
        .or_default();
    let current = *slot;
    *slot = slot.saturating_add(1);
    Some(current)
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
    let server_paths = state
        .borrow()
        .net
        .servers
        .values()
        .filter_map(|server| {
            let server = server.borrow();
            (server.owner_worker == Some(worker_id))
                .then(|| server.path.clone())
                .flatten()
        })
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
    for path in server_paths {
        state.borrow_mut().net.paths.remove(&path);
        let _ = std::fs::remove_file(path);
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
    // A disconnected worker owns no remaining host registrations. Remove the
    // closed records now so a later fork starts from a clean listener/socket
    // set instead of traversing stale descriptors.
    state
        .borrow_mut()
        .net
        .servers
        .retain(|_, server| server.borrow().owner_worker != Some(worker_id));
    state.borrow_mut().net.sockets.retain(|_, socket| {
        let socket = socket.borrow();
        !(socket.server_id.is_some_and(|id| server_ids.contains(&id))
            || socket
                .local
                .is_some_and(|address| server_addrs.contains(&address))
            || socket
                .peer
                .is_some_and(|address| server_addrs.contains(&address)))
    });
    let stdio = state
        .borrow()
        .cluster
        .workers
        .get(&worker_id)
        .and_then(|worker| execute::get_property_result(&worker.object, "process").ok())
        .and_then(|process| execute::get_property_result(&process, "stdio").ok());
    let stdio_sockets = match stdio {
        Some(Value::Array(values)) => (0..values.logical_len())
            .map(|index| execute::get_property(&Value::Array(values.clone()), &index.to_string()))
            .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    for socket in stdio_sockets {
        let _ = crate::modules::net::socket_destroy(state, Some(&socket), &[]);
    }
    let fd_sockets = state
        .borrow()
        .net
        .sockets
        .values()
        .filter_map(|socket| {
            let socket = socket.borrow();
            matches!(
                execute::get_property(&socket.js, crate::modules::net::PIPE_FD_PROP),
                Value::Number(fd) if fd == 4.0
            )
            .then(|| socket.js.clone())
        })
        .collect::<Vec<_>>();
    for socket in fd_sockets {
        let _ = crate::modules::net::socket_destroy(state, Some(&socket), &[]);
    }
}

/// Complete a primary-side `Worker.disconnect()` after the worker's own
/// referenced handles have closed. Disconnecting IPC must not tear down those
/// handles: Node keeps the worker alive until its event loop becomes idle.
pub(crate) fn finalize_disconnected_workers(state: &Rc<RefCell<HostState>>) {
    // The disconnect notification is queued by `Worker.disconnect()`. Give
    // that turn a chance to run before a zero-handle worker is finalized, so
    // observers always see `disconnect` before `exit`.
    if !state.borrow().event_loop.microtasks.borrow().is_empty() {
        return;
    }
    // A worker's process `disconnect` event follows closure of its listening
    // servers, but may precede closure of accepted sockets. Deliver it once
    // the server records have emitted their close events.
    let child_disconnect_ids = {
        let guard = state.borrow();
        guard
            .cluster
            .workers
            .iter()
            .filter_map(|(id, worker)| {
                let no_servers = !guard
                    .net
                    .servers
                    .values()
                    .any(|server| server.borrow().owner_worker == Some(*id));
                (worker.pending_child_disconnect && no_servers).then_some(*id)
            })
            .collect::<Vec<_>>()
    };
    for id in child_disconnect_ids {
        let Some(worker) = state.borrow().cluster.worker_object(id) else {
            continue;
        };
        if let Some(entry) = state.borrow_mut().cluster.workers.get_mut(&id) {
            entry.pending_child_disconnect = false;
        }
        let previous = state.borrow().cluster.worker_context;
        crate::modules::cluster::set_worker_mode(state, id, &worker, true);
        state.borrow_mut().cluster.worker_context = Some(id);
        // The control-channel disconnect is observed by the worker's
        // `cluster.worker` EventEmitter.  The worker-side process may also
        // expose a disconnect event, but it is a distinct target; dispatch
        // the cluster worker notification here so listeners installed by the
        // child entry script run before the worker is finalized.
        let _ = crate::modules::cluster::emit(
            state,
            Some(&worker),
            &[Value::String("disconnect".into())],
        );
        // `process.on('disconnect')` is a separate EventEmitter from
        // `cluster.worker`; preserve both observable targets for workers that
        // use the process-level API.
        let _ = crate::modules::process::emit(state, &[Value::String("disconnect".into())]);
        // A worker-side listener may call process.exit(code) while handling
        // this notification.  Capture that logical child status before
        // restoring the primary process view, then deliver the corresponding
        // parent-side exit events instead of allowing the status to be lost
        // (or to terminate the primary runner).
        let child_exit = state.borrow_mut().process.exit_code.take();
        if let Some(code) = child_exit {
            if let Some(entry) = state.borrow_mut().cluster.workers.get_mut(&id) {
                entry.dead = true;
                entry.pending_exit = None;
            }
            let _ = execute::set_property_in_place(&worker, "state", Value::String("dead".into()));
            if let Ok(process) = execute::get_property_result(&worker, "process") {
                let _ = execute::set_property_in_place(&process, "exitCode", Value::Number(code as f64));
                let _ = execute::set_property_in_place(&process, "signalCode", Value::Null);
            }
            state.borrow_mut().cluster.worker_context = previous;
            crate::modules::cluster::set_worker_mode(state, id, &worker, false);
            let _ = crate::modules::cluster::emit(
                state,
                Some(&worker),
                &[
                    Value::String("exit".into()),
                    Value::Number(code as f64),
                    Value::Null,
                ],
            );
            if let Some(module) = state.borrow().cluster.module.clone() {
                let _ = crate::modules::events::method_emit(
                    state,
                    Some(&module),
                    &[
                        Value::String("exit".into()),
                        worker.clone(),
                        Value::Number(code as f64),
                        Value::Null,
                    ],
                );
            }
        }
        if child_exit.is_none() {
            state.borrow_mut().cluster.worker_context = previous;
            crate::modules::cluster::set_worker_mode(state, id, &worker, false);
        }
    }

    let ids = {
        let guard = state.borrow();
        guard
            .cluster
            .workers
            .iter()
            .filter_map(|(id, worker)| {
                if worker.connected || worker.dead || worker.pending_exit.is_some() {
                    return None;
                }
                let live = guard.net.servers.values().any(|server| {
                    let server = server.borrow();
                    server.owner_worker == Some(*id)
                        && server.listening
                        && server.refed
                        && !server.closed
                }) || guard.net.sockets.values().any(|socket| {
                    let socket = socket.borrow();
                    socket.owner_worker == Some(*id)
                        && socket.refed
                        && socket.state != crate::modules::net::SocketState::Closed
                });
                (!live).then_some(*id)
            })
            .collect::<Vec<_>>()
    };
    for id in ids {
        let Some(worker) = state.borrow().cluster.worker_object(id) else {
            continue;
        };
        let has_exit_listener = state
            .borrow()
            .cluster
            .workers
            .get(&id)
            .is_some_and(|entry| entry.listeners.contains_key("exit"));
        if let Some(entry) = state.borrow_mut().cluster.workers.get_mut(&id) {
            entry.dead = true;
            if !has_exit_listener {
                entry.pending_exit = Some((0, None));
            }
        }
        let _ = execute::set_property_in_place(&worker, "state", Value::String("dead".into()));
        let _ = emit(
            state,
            Some(&worker),
            &[
                Value::String("exit".into()),
                Value::Number(0.0),
                Value::Null,
            ],
        );
        if let Some(module) = state.borrow().cluster.module.clone() {
            let _ = crate::modules::events::method_emit(
                state,
                Some(&module),
                &[
                    Value::String("exit".into()),
                    worker,
                    Value::Number(0.0),
                    Value::Null,
                ],
            );
        }
    }
}

/// Whether a worker disconnect transition still needs a pump turn.  The
/// primary-side `Worker.disconnect()` first queues parent notifications and
/// then marks the child-side process notification pending.  Neither state is
/// represented by a socket/timer, so the event-loop liveness check must count
/// it explicitly or the loop can go idle immediately after draining the
/// parent microtask.
pub(crate) fn has_pending_disconnect(state: &Rc<RefCell<HostState>>) -> bool {
    state
        .borrow()
        .cluster
        .workers
        .values()
        .any(|worker| {
            // A terminal worker may retain this marker so a listener added
            // synchronously after fork() can still observe the late
            // disconnect. Once the worker is already dead, however, the
            // marker cannot represent referenced work: keeping it in the
            // liveness predicate would leave a worker with no listeners
            // holding the primary event loop open forever.
            worker.pending_child_disconnect || (worker.pending_disconnect && !worker.dead)
        })
}

/// Stop worker-owned listeners during a primary-side disconnect while leaving
/// accepted sockets alive so their normal EOF/close path remains observable.
fn close_worker_servers(state: &Rc<RefCell<HostState>>, worker_id: u64) {
    let servers = state
        .borrow()
        .net
        .servers
        .values()
        .filter_map(|server| {
            (server.borrow().owner_worker == Some(worker_id))
                .then(|| server.borrow().js.clone())
        })
        .collect::<Vec<_>>();
    for server in servers {
        let _ = crate::modules::net::server_close(state, Some(&server), &[]);
    }
}

pub fn close_worker_net_binding(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(Value::Number(worker_id)) = args.first() {
        close_worker_net(state, *worker_id as u64);
    }
    Ok(Value::Undefined)
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
    let object = {
        let guard = state.borrow();
        guard
            .cluster
            .workers
            .get(&id)
            .map(|worker| worker.object.clone())
    }
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
        let child = { state.borrow().cluster.worker_context == Some(id) };
        {
            let mut guard = state.borrow_mut();
            let Some(worker) = guard.cluster.workers.get_mut(&id) else {
                return Ok(obj);
            };
            let listeners = if child {
                &mut worker.child_listeners
            } else {
                &mut worker.listeners
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
                if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
                    worker.pending_disconnect = false;
                }
                let terminal = state
                    .borrow()
                    .cluster
                    .workers
                    .get(&id)
                    .is_some_and(|worker| worker.dead || worker.pending_exit.is_some());
                if terminal {
                    remove_worker(state, id, &obj);
                }
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
        if name == "listening" {
            let pending = state
                .borrow_mut()
                .cluster
                .workers
                .get_mut(&id)
                .map(|worker| std::mem::take(&mut worker.pending_listening))
                .unwrap_or_default();
            for address in pending {
                let info = listening_info(&address);
                state.borrow_mut().event_loop.queue_microtask_with_receiver(
                    crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
                    vec![Value::String("listening".into()), info],
                    obj.clone(),
                );
            }
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
    let child = { state.borrow().cluster.worker_context == Some(id) };
    let callbacks = {
        let guard = state.borrow();
        guard
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
            .unwrap_or_default()
    };
    let present = !callbacks.is_empty();
    if name == "online" {
        let _ = execute::set_property_in_place(&obj, "state", Value::String("online".into()));
    } else if name == "listening" {
        let _ = execute::set_property_in_place(&obj, "state", Value::String("listening".into()));
    }
    for cb in callbacks {
        execute::call(&cb, &obj, &args[1..])?;
    }
    if name == "online" {
        let module = { state.borrow().cluster.module.clone() };
        if let Some(module) = module {
            let _ = crate::modules::events::method_emit(
                state,
                Some(&module),
                &[Value::String("online".into()), obj.clone()],
            );
        }
    }
    Ok(Value::Boolean(present))
}
pub fn disconnect(
    state: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (id, obj) = worker(state, r)?;
    let child_call = state.borrow().cluster.worker_context == Some(id);
    let process_disconnect = r.is_some_and(|receiver| {
        matches!(
            execute::get_property(receiver, "\0clusterProcessSender"),
            Value::Boolean(true)
        )
    });
    // `worker.process.disconnect()` closes the control channel without
    // delivering a child-side `process` disconnect event.  The child is then
    // forcibly retired with status 0; only the parent-facing Worker exit
    // transition is observable.  Keep this as a receiver fact so
    // `cluster.worker.disconnect()` retains its distinct event semantics.
    if process_disconnect && !child_call {
        finish_worker_callback_exit(state, id, 0);
        if let Some(cb) = args.first().filter(|v| quench_runtime::is_callable(v)) {
            state
                .borrow()
                .event_loop
                .queue_microtask(cb.clone(), Vec::new());
        }
        return Ok(obj);
    }
    let parent_has_disconnect = child_call
        && state
            .borrow()
            .cluster
            .workers
            .get(&id)
            .is_some_and(|worker| worker.listeners.contains_key("disconnect"));
    if let Some(w) = state.borrow_mut().cluster.workers.get_mut(&id) {
        w.connected = false;
        let _ = execute::set_property_in_place(&obj, "exitedAfterDisconnect", Value::Boolean(true));
        let _ = execute::set_property_in_place(&obj, "state", Value::String("disconnected".into()));
        if let Ok(process) = execute::get_property_result(&obj, "process") {
            let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
        }
    }
    if child_call {
        if parent_has_disconnect {
            state.borrow().event_loop.queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
                vec![Value::String("disconnect".into())],
                obj.clone(),
                0,
            );
        }
        let _ = emit(state, Some(&obj), &[Value::String("disconnect".into())]);
        if let Some(module) = state.borrow().cluster.module.clone() {
            state.borrow().event_loop.queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_EVENTS_EMIT),
                vec![Value::String("disconnect".into()), obj.clone()],
                module,
                0,
            );
        }
    } else {
        // Node delivers Worker disconnect asynchronously.  Queue both the
        // worker and cluster-level observers so listeners installed after
        // worker.disconnect() can still observe the transition.
        let scope = state.borrow().cluster.process_scope();
        state.borrow().event_loop.queue_microtask_with_receiver_scope(
            crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
            vec![Value::String("disconnect".into())],
            obj.clone(),
            scope,
        );
        if let Some(module) = state.borrow().cluster.module.clone() {
            state.borrow().event_loop.queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_EVENTS_EMIT),
                vec![Value::String("disconnect".into()), obj.clone()],
                module,
                scope,
            );
        }
        close_worker_servers(state, id);
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.pending_child_disconnect = true;
        }
    }
    let (child_result, child_exit) = if child_call {
        let previous_context = state.borrow().cluster.worker_context;
        set_worker_mode(state, id, &obj, true);
        state.borrow_mut().cluster.worker_context = Some(id);
        let result = emit(state, Some(&obj), &[Value::String("disconnect".into())]);
        let exit = state.borrow().process.exit_code;
        state.borrow_mut().process.exit_code = None;
        state.borrow_mut().cluster.worker_context = previous_context;
        set_worker_mode(state, id, &obj, false);
        (result, exit)
    } else {
        (Ok(Value::Undefined), None)
    };
    if child_call {
        let code = child_exit.unwrap_or(if child_result.is_ok() { 0 } else { 1 });
        close_worker_net(state, id);
        let parent_has_exit = state
            .borrow()
            .cluster
            .workers
            .get(&id)
            .is_some_and(|worker| worker.listeners.contains_key("exit"));
        if let Some(w) = state.borrow_mut().cluster.workers.get_mut(&id) {
            w.dead = true;
            // A worker can disconnect while fork() is still re-entering
            // its script; retain terminal events until the parent adds
            // listeners after fork() returns.
            if !parent_has_disconnect {
                w.pending_disconnect = true;
            }
            if !parent_has_exit {
                w.pending_exit = Some((code, None));
            }
        }
        // Terminal worker events belong to the parent cluster observer even
        // when disconnect() was invoked from the re-entered worker script.
        // Restore primary context before dispatch so `emit` selects the
        // parent's listener set rather than the child's.
        state.borrow_mut().cluster.worker_context = None;
        if parent_has_exit {
            state.borrow().event_loop.queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
                vec![
                    Value::String("exit".into()),
                    Value::Number(code as f64),
                    Value::Null,
                ],
                obj.clone(),
                0,
            );
        }
        let module = state.borrow().cluster.module.clone();
        if let Some(module) = module {
            state.borrow().event_loop.queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_EVENTS_EMIT),
                vec![
                    Value::String("exit".into()),
                    obj.clone(),
                    Value::Number(code as f64),
                    Value::Null,
                ],
                module,
                0,
            );
        }
    }
    if let Some(cb) = args.first().filter(|v| quench_runtime::is_callable(v)) {
        state
            .borrow()
            .event_loop
            .queue_microtask(cb.clone(), Vec::new());
    }
    Ok(obj)
}

/// Complete a `process.exit()` raised by a callback that was registered by a
/// logical cluster worker. Cluster workers share the host VM, so the normal
/// top-level unwind must be consumed and converted into the parent-side
/// Worker exit event instead of terminating the primary runner.
pub(crate) fn finish_worker_callback_exit(
    state: &Rc<RefCell<HostState>>,
    id: u64,
    code: i32,
) {
    let Some(worker) = state.borrow().cluster.worker_object(id) else {
        return;
    };
    let parent_scope = state
        .borrow()
        .cluster
        .worker_parent_scope(id)
        .unwrap_or(0);
    close_worker_net(state, id);
    let has_exit_listener = state
        .borrow()
        .cluster
        .workers
        .get(&id)
        .is_some_and(|entry| entry.listeners.contains_key("exit"));
    if let Some(entry) = state.borrow_mut().cluster.workers.get_mut(&id) {
        entry.connected = false;
        entry.dead = true;
        entry.pending_child_disconnect = false;
        entry.pending_disconnect = false;
        if !has_exit_listener {
            entry.pending_exit = Some((code, None));
        } else {
            entry.pending_exit = None;
        }
    }
    let _ = execute::set_property_in_place(&worker, "state", Value::String("dead".into()));
    if let Ok(process) = execute::get_property_result(&worker, "process") {
        let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
        let _ = execute::set_property_in_place(&process, "exitCode", Value::Number(code as f64));
        let _ = execute::set_property_in_place(&process, "signalCode", Value::Null);
    }
    if has_exit_listener {
        state
            .borrow()
            .event_loop
            .queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_CLUSTER_WORKER_EMIT),
                vec![
                    Value::String("exit".into()),
                    Value::Number(code as f64),
                    Value::Null,
                ],
                worker.clone(),
                parent_scope,
            );
    }
    if let Some(module) = state.borrow().cluster.module.clone() {
        state
            .borrow()
            .event_loop
            .queue_microtask_with_receiver_scope(
                crate::host::capability(SPEC_EVENTS_EMIT),
                vec![
                    Value::String("exit".into()),
                    worker,
                    Value::Number(code as f64),
                    Value::Null,
                ],
                module,
                parent_scope,
            );
    }
}
fn remove_worker(state: &Rc<RefCell<HostState>>, id: u64, worker: &Value) {
    let mut host = state.borrow_mut();
    host.cluster.workers.remove(&id);
    host.cluster.worker_listen_slots.remove(&id);
    drop(host);
    remove_public_worker(state, id);
    let _ = execute::set_property_in_place(worker, "state", Value::String("dead".into()));
}

fn remove_public_worker(state: &Rc<RefCell<HostState>>, id: u64) {
    let Some(module) = state.borrow().cluster.module.clone() else {
        return;
    };
    let Ok(workers) = execute::get_property_result(&module, "workers") else {
        return;
    };
    let (workers, _) = execute::delete_property(workers, &id.to_string());
    let _ = execute::set_property_in_place(&module, "workers", workers);
}

/// Convert an uncaught exception in a forked logical process into that
/// process's terminal exit instead of unwinding the embedding VM.
pub(crate) fn fail_fork_process(
    state: &Rc<RefCell<HostState>>,
    scope: u64,
    code: i32,
) -> Result<bool, VmError> {
    let child = state.borrow_mut().cluster.take_fork_process(scope);
    let Some(child) = child else {
        return Ok(false);
    };
    let ids = state
        .borrow()
        .cluster
        .workers
        .iter()
        .filter_map(|(id, worker)| (worker.scope == scope).then_some(*id))
        .collect::<Vec<_>>();
    for id in ids {
        close_worker_net(state, id);
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.connected = false;
            worker.dead = true;
        }
    }
    execute::set_property_in_place(&child, "connected", Value::Boolean(false));
    execute::set_property_in_place(&child, "exitCode", Value::Number(code as f64));
    execute::set_property_in_place(&child, "signalCode", Value::Null);
    // Mark the terminal transition before queued spawn-output work runs. The
    // output callback must not emit a second synthetic exit(0) after this
    // error path has already delivered exit(code)/close to the parent.
    execute::set_property_in_place(&child, "\0childTerminated", Value::Boolean(true));
    let stderr = execute::get_property(&child, "stderr");
    if let Value::String(text) = execute::get_property(&child, "\0forkStderr") {
        if !text.is_empty() && matches!(stderr, Value::Object(_) | Value::ObjectAlias(_)) {
            crate::modules::events::method_emit(
                state,
                Some(&stderr),
                &[Value::String("data".into()), Value::String(text)],
            )?;
        }
    }
    if matches!(stderr, Value::Object(_) | Value::ObjectAlias(_)) {
        for event in ["end", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&stderr),
                &[Value::String(event.into())],
            )?;
        }
    }
    for event in ["exit", "close"] {
        crate::modules::events::method_emit(
            state,
            Some(&child),
            &[
                Value::String(event.into()),
                Value::Number(code as f64),
                Value::Null,
            ],
        )?;
    }
    Ok(true)
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
    let handle = args
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .cloned();
    let child_context = state.borrow().cluster.worker_context == Some(id);
    if child_context {
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.pending_messages.push(message.clone());
        }
        return Ok(Value::Boolean(true));
    }
    let previous_context = state.borrow().cluster.worker_context;
    if let (Some(handle), Some(scope)) = (handle.as_ref(), state.borrow().cluster.worker_scope(id))
    {
        crate::modules::net::transfer_handle_scope(state, handle, scope);
    }
    set_worker_mode(state, id, &obj, true);
    state.borrow_mut().cluster.worker_context = Some(id);
    let mut process_args = vec![Value::String("message".into()), message];
    if let Some(handle) = handle {
        process_args.push(handle);
    }
    let process_result = crate::modules::process::emit(state, &process_args);
    state.borrow_mut().cluster.worker_context = previous_context;
    set_worker_mode(state, id, &obj, false);
    let child_exit = state.borrow_mut().process.exit_code.take();
    if let Some(code) = child_exit {
        close_worker_net(state, id);
        if let Some(worker) = state.borrow_mut().cluster.workers.get_mut(&id) {
            worker.connected = false;
            worker.dead = true;
        }
        if let Ok(process) = execute::get_property_result(&obj, "process") {
            let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
            let _ = execute::set_property_in_place(&process, "channel", Value::Null);
            let _ =
                execute::set_property_in_place(&process, "exitCode", Value::Number(code as f64));
            let _ = emit(state, Some(&process), &[Value::String("close".into())]);
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
    }
    if process_result.is_err() && child_exit.is_none() {
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
    if let Some(callback) = args.iter().find(|value| quench_runtime::is_callable(value)) {
        execute::call(callback, &Value::Undefined, &[Value::Null])?;
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
    let (active, callbacks, worker_object) = {
        let guard = state.borrow();
        let Some(worker) = guard.cluster.workers.get(&id) else {
            return Ok(Value::Boolean(false));
        };
        (
            guard.cluster.worker_context == Some(id),
            worker.listeners.get("message").cloned().unwrap_or_default(),
            worker.object.clone(),
        )
    };
    if !active {
        return Ok(Value::Boolean(false));
    }
    if callbacks.is_empty() {
        let mut guard = state.borrow_mut();
        let Some(worker) = guard.cluster.workers.get_mut(&id) else {
            return Ok(Value::Boolean(false));
        };
        worker.pending_messages.push(message.clone());
    } else {
        for callback in callbacks {
            state.borrow().event_loop.queue_microtask_with_receiver(
                callback,
                vec![message.clone()],
                worker_object.clone(),
            );
        }
    }
    if let Some(module) = state.borrow().cluster.module.clone() {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&module),
            &[
                Value::String("message".into()),
                worker_object,
                message.clone(),
            ],
        );
    }
    let connected = state
        .borrow()
        .cluster
        .workers
        .get(&id)
        .is_some_and(|worker| worker.connected && !worker.dead);
    if !connected {
        return Ok(Value::Boolean(false));
    }
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
        // Keep the host record until the worker-side disconnect turn has run:
        // timers and other scoped callbacks still need the event-scope to
        // worker mapping during teardown.  The public collection, however,
        // becomes empty as soon as `cluster.disconnect()` starts, matching
        // the state observed by its completion callback.
        remove_public_worker(state, id);
    }
    if let Some(cb) = args.first().filter(|v| quench_runtime::is_callable(v)) {
        state
            .borrow()
            .event_loop
            .queue_microtask(cb.clone(), Vec::new());
    }
    Ok(Value::Undefined)
}

fn listening_info(address: &Value) -> Value {
    let family = execute::get_property(address, "family");
    let address_type = matches!(family, Value::String(ref family) if family == "IPv6")
        .then_some(6.0)
        .unwrap_or(4.0);
    host_api::object(vec![
        ("address".into(), execute::get_property(address, "address")),
        ("addressType".into(), Value::Number(address_type)),
        ("fd".into(), Value::Undefined),
        ("port".into(), execute::get_property(address, "port")),
    ])
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
