//! `node:cluster`: a primary forks worker processes over the child-process IPC channel
//! and shares listening servers with them, distributing connections round-robin
//! (`SCHED_RR`) or handing each worker the listening handle (`SCHED_NONE`).
//!
//! Which side a process is follows Node: a process started with `NODE_UNIQUE_ID` is a
//! worker ([`child`]), any other the primary ([`primary`]). Both speak Node's internal
//! `NODE_CLUSTER` messages, where a reply names the `seq` it acknowledges.
pub(crate) mod child;
pub(crate) mod primary;

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::{BTreeMap, HashMap, VecDeque};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The environment variable naming a worker's id (Node's cluster child marker).
pub(crate) const WORKER_ID_VARIABLE: &str = "NODE_UNIQUE_ID";
const COMMAND: &str = "NODE_CLUSTER";
/// Node's scheduling policies.
const SCHED_NONE: f64 = 1.0;
const SCHED_RR: f64 = 2.0;
const POLICY_VARIABLE: &str = "NODE_CLUSTER_SCHED_POLICY";

/// What an acknowledgement of a sent message resumes.
pub(crate) enum Pending {
    /// A worker's `queryServer`: the server waiting for the primary's reply.
    QueryServer { server: RootId, message: RootId },
    /// A worker's `exitedAfterDisconnect`: disconnect once acknowledged.
    Disconnect,
    /// A round-robin connection handed to a worker.
    Handoff { key: String, worker: u32, socket: RootId },
}

/// A listening server the primary shares, by Node's handle key.
pub(crate) struct SharedServer {
    /// The primary's server (round-robin) or bare listening handle (shared).
    owner: RootId,
    round_robin: bool,
    workers: Vec<u32>,
    /// Workers whose add waits for the server to listen: (worker, queryServer message).
    waiting: Vec<(u32, RootId)>,
    listening: bool,
    /// Round-robin connections not yet handed off, and workers ready for one.
    connections: VecDeque<RootId>,
    free: VecDeque<u32>,
}

#[derive(Default)]
pub(crate) struct State {
    sequence: u32,
    pending: HashMap<u32, Pending>,
    module: Option<RootId>,
    worker_prototype: Option<RootId>,
    // Primary.
    next_id: u32,
    initialized: bool,
    policy_frozen: Option<f64>,
    workers: BTreeMap<u32, RootId>,
    servers: HashMap<String, SharedServer>,
    disconnect_callbacks: Vec<RootId>,
    // Worker.
    worker: Option<RootId>,
    worker_servers: HashMap<String, RootId>,
    indexes: HashMap<String, (usize, Vec<usize>)>,
}

fn with_state<T>(c: &mut Context<'_>, f: impl FnOnce(&mut State) -> T) -> T {
    let shared = c.host_mut().shared_state();
    let mut shared = shared.borrow_mut();
    f(&mut shared.cluster)
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.set_property_rooted(object, key, value, object)?;
    Ok(())
}

fn call(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}

fn function(c: &mut Context<'_>, operation: &str, name: &str, data: RootId) -> Result<RootId, RootedError> {
    c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)
}

fn number(c: &mut Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process module is not initialized"))
}

fn next_tick(c: &mut Context<'_>, callback: RootId, receiver: RootId, args: &[RootId]) -> Result<(), RootedError> {
    super::shared_event_loop::queue_next_tick(c, callback, receiver, args)
}

/// Whether this process is a cluster worker (the module's side).
pub(crate) fn is_worker() -> bool {
    std::env::var_os(WORKER_ID_VARIABLE).is_some()
}

/// Whether this process has set itself up as a cluster worker (`_setupWorker` ran).
pub(crate) fn worker_active(c: &mut Context<'_>) -> bool {
    with_state(c, |state| state.worker.is_some())
}

pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(module) = with_state(c, |state| state.module) {
        return Ok(module);
    }
    let events = crate::host::shared_vm::commonjs::events_module(c)?;
    let cluster = c.construct_rooted(events, events, &[])?;
    let worker = worker_class(c, events)?;
    put(c, cluster, "Worker", worker)?;
    if is_worker() {
        child::install(c, cluster)?;
    } else {
        primary::install(c, cluster)?;
    }
    let cluster = c.retain(cluster)?;
    with_state(c, |state| state.module = Some(cluster));
    Ok(cluster)
}

/// The scheduling policy Node derives from `NODE_CLUSTER_SCHED_POLICY` and the platform.
fn default_policy() -> f64 {
    match std::env::var(POLICY_VARIABLE).as_deref() {
        Ok("rr") => SCHED_RR,
        Ok("none") => SCHED_NONE,
        _ if cfg!(windows) => SCHED_NONE,
        _ => SCHED_RR,
    }
}

/// The methods each side gives `Worker.prototype`.
const SHARED_METHODS: [(&str, &str); 4] = [
    ("kill", "clusterWorkerKill"),
    ("send", "clusterWorkerSend"),
    ("isDead", "clusterWorkerIsDead"),
    ("isConnected", "clusterWorkerIsConnected"),
];

/// Node's `internal/cluster/worker`: an EventEmitter over its process.
fn worker_class(c: &mut Context<'_>, events: RootId) -> Result<RootId, RootedError> {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("clusterWorker"), "Worker")?;
    let prototype = get(c, constructor, "prototype")?;
    let emitter_prototype = get(c, events, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    c.set_prototype_rooted(constructor, events)?;
    let undefined = c.undefined();
    let side: &[(&str, &str)] = if is_worker() { &child::WORKER_METHODS } else { &primary::WORKER_METHODS };
    for (name, operation) in SHARED_METHODS.iter().chain(side) {
        let method = function(c, operation, name, undefined)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, method, true, false, true)?;
    }
    let prototype = c.retain(prototype)?;
    with_state(c, |state| state.worker_prototype = Some(prototype));
    Ok(constructor)
}

/// `new Worker({ id, process, state })`.
pub(crate) fn worker_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let receiver = match c.new_target_rooted() {
        Some(_) => this,
        None => {
            let prototype = with_state(c, |state| state.worker_prototype)
                .ok_or_else(|| RootedError::host("cluster Worker prototype missing"))?;
            let object = c.object_rooted()?;
            c.set_prototype_rooted(object, prototype)?;
            object
        }
    };
    let events = crate::host::shared_vm::commonjs::events_module(c)?;
    c.call_rooted(events, receiver, &[])?;
    let undefined = c.undefined();
    let options = args.first().copied().filter(|options| c.is_object_rooted(*options).unwrap_or(false));
    put(c, receiver, "exitedAfterDisconnect", undefined)?;
    let state = match options {
        Some(options) => get(c, options, "state")?,
        None => undefined,
    };
    let state = if c.truthy_rooted(state)? { state } else { c.string_rooted("none") };
    put(c, receiver, "state", state)?;
    let id = match options {
        Some(options) => get(c, options, "id")?,
        None => undefined,
    };
    let id = number(c, id).map_or(0.0, |id| f64::from(id as i32));
    let id = c.number(id);
    put(c, receiver, "id", id)?;
    let process = match options {
        Some(options) => get(c, options, "process")?,
        None => undefined,
    };
    if c.truthy_rooted(process)? {
        put(c, receiver, "process", process)?;
        for (event, operation) in [("error", "clusterWorkerForwardError"), ("message", "clusterWorkerForwardMessage")] {
            let forward = function(c, operation, "", receiver)?;
            let event = c.string_rooted(event);
            call(c, process, "on", &[event, forward])?;
        }
    }
    Ok(receiver)
}

fn forward(c: &mut Context<'_>, event: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let worker = c.host_function_data()?;
    let event = c.string_rooted(event);
    let mut arguments = vec![event];
    arguments.extend(args.iter().copied().take(2));
    call(c, worker, "emit", &arguments)
}

pub(crate) fn worker_forward_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    forward(c, "error", args)
}

pub(crate) fn worker_forward_message(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    forward(c, "message", args)
}

pub(crate) fn worker_kill(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    call(c, this, "destroy", args)
}

pub(crate) fn worker_send(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let process = get(c, this, "process")?;
    call(c, process, "send", args)
}

pub(crate) fn worker_is_dead(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let process = get(c, this, "process")?;
    let mut dead = false;
    for name in ["exitCode", "signalCode"] {
        let value = get(c, process, name)?;
        dead |= !c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined());
    }
    Ok(c.boolean(dead))
}

pub(crate) fn worker_is_connected(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let process = get(c, this, "process")?;
    get(c, process, "connected")
}

/// Node's `sendHelper`: a `NODE_CLUSTER` message with the next `seq`, its handle, and
/// what its acknowledgement resumes.
fn send(
    c: &mut Context<'_>,
    target: RootId,
    fields: &[(&str, RootId)],
    handle: Option<RootId>,
    pending: Option<Pending>,
) -> Result<bool, RootedError> {
    let connected = get(c, target, "connected")?;
    if !c.truthy_rooted(connected)? {
        return Ok(false);
    }
    let message = c.object_rooted()?;
    let command = c.string_rooted(COMMAND);
    put(c, message, "cmd", command)?;
    for (name, value) in fields {
        put(c, message, name, *value)?;
    }
    let sequence = with_state(c, |state| {
        let sequence = state.sequence;
        state.sequence += 1;
        if let Some(pending) = pending {
            state.pending.insert(sequence, pending);
        }
        sequence
    });
    let sequence = c.number(f64::from(sequence));
    put(c, message, "seq", sequence)?;
    let sent = match handle {
        Some(handle) => {
            let options = c.object_rooted()?;
            let keep = c.boolean(true);
            put(c, options, "keepOpen", keep)?;
            call(c, target, "send", &[message, handle, options])?
        }
        None => call(c, target, "send", &[message])?,
    };
    c.truthy_rooted(sent)
}

/// Copy a message's own enumerable fields onto `target` (`{ ...message }`).
fn assign(c: &mut Context<'_>, target: RootId, source: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let assign = get(c, object, "assign")?;
    c.call_rooted(assign, object, &[target, source])?;
    Ok(())
}

/// Node's `internal(worker, cb)`: a `NODE_CLUSTER` message, routed to the callback its
/// `ack` names or to the side's handler.
fn route(c: &mut Context<'_>, args: &[RootId]) -> Result<Option<(RootId, Option<RootId>, Option<Pending>)>, RootedError> {
    let Some(message) = args.first().copied().filter(|message| c.is_object_rooted(*message).unwrap_or(false)) else {
        return Ok(None);
    };
    let command = get(c, message, "cmd")?;
    if c.string_text(command)?.as_deref() != Some(COMMAND) {
        return Ok(None);
    }
    let ack = get(c, message, "ack")?;
    let pending = match number(c, ack) {
        Some(ack) => with_state(c, |state| state.pending.remove(&(ack as u32))),
        None => None,
    };
    Ok(Some((message, args.get(1).copied(), pending)))
}

fn text(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = get(c, object, name)?;
    c.string_text(value)
}
