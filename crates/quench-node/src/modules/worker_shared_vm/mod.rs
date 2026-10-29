//! `worker_threads` and the global `MessageChannel`/`MessagePort`.
//!
//! A channel is two process-wide endpoints (`ports`); each VM keeps the ports it holds in
//! its `State`, and a transferred port's endpoint moves with the message that carries it.
//! Messages are structured-serialized (`quench_runtime::SerializedValue`), so they cross
//! between heaps on different threads. A `Worker` runs a complete Node environment on its
//! own thread (`thread`). The event loop delivers port messages, port closure and worker
//! lifecycle events through `messagingDrain`, one event per call.

mod js;
mod ports;
mod thread;

use crate::host::NodeHost;
use ports::{Endpoint, Message};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::BTreeMap;
use std::sync::Arc;
pub(crate) use thread::{capture_error, capture_fatal, WorkerContext};

type Context<'a> = NativeContext<'a, NodeHost>;

/// A port this VM holds: its endpoint and its JavaScript object, retained while open.
struct LocalPort {
    endpoint: Arc<Endpoint>,
    object: RootId,
    started: bool,
    referenced: bool,
    /// Closed here; the 'close' event is still due.
    closing: bool,
}

/// A worker this VM started, until its 'exit' has been delivered.
struct LocalWorker {
    control: Arc<thread::WorkerControl>,
    object: RootId,
    referenced: bool,
    online_delivered: bool,
}

/// Per-VM messaging state.
#[derive(Default)]
pub(crate) struct State {
    ports: BTreeMap<u64, LocalPort>,
    workers: Vec<LocalWorker>,
    next_port: u64,
    /// The classes and dispatch functions `js::MESSAGING_JS` returns, built once per VM.
    api: Option<RootId>,
    /// Set when this VM is a worker.
    pub(crate) context: Option<WorkerContext>,
}

impl State {
    /// Forget guest handles when the VM heap is replaced; endpoints close with their ports.
    pub(crate) fn reset(&mut self) {
        for port in std::mem::take(&mut self.ports).into_values() {
            port.endpoint.close();
        }
        self.workers.clear();
        self.api = None;
    }

    fn has_work(&self) -> bool {
        self.ports.values().any(|port| {
            port.closing
                || (port.started && (port.referenced || port.endpoint.has_messages()))
                || (port.referenced && port.endpoint.peer_closed())
        }) || self.workers.iter().any(|worker| worker.referenced)
            || self.context.as_ref().is_some_and(|context| context.control.terminating())
    }
}

fn state(c: &mut Context<'_>) -> std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>> {
    c.host_mut().shared_state()
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}

fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true).map(drop)
}

/// The messaging API object, built on first use.
fn api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(api) = state(c).borrow().messaging.api {
        return Ok(api);
    }
    let native = c.object_rooted()?;
    for (name, operation) in [
        ("channel", "messagingChannel"),
        ("post", "messagingPost"),
        ("start", "messagingStart"),
        ("close", "messagingClose"),
        ("ref", "messagingRef"),
        ("hasRef", "messagingHasRef"),
        ("receive", "messagingReceive"),
        ("workerStart", "workerStart"),
        ("workerRef", "messagingWorkerRef"),
        ("workerTerminate", "messagingWorkerTerminate"),
        ("workerExited", "workerExited"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        set(c, native, name, function)?;
    }
    let events = crate::host::shared_vm::commonjs::events_module(c)?;
    let stream = crate::host::shared_vm::commonjs::stream_module(c)?;
    let readable = get(c, stream, "Readable")?;
    let path = crate::host::shared_vm::commonjs::path_module(c)?;
    let url = crate::host::shared_vm::commonjs::url_module(c)?;
    let file_url_to_path = get(c, url, "fileURLToPath")?;
    let factory = c.evaluate_script_rooted(js::MESSAGING_JS, "node:internal/worker/io.js")?;
    let undefined = c.undefined();
    let api = c.call_rooted(factory, undefined, &[native, events, path, file_url_to_path, readable])?;
    let api = c.retain(api)?;
    state(c).borrow_mut().messaging.api = Some(api);
    Ok(api)
}

/// `MessagePort`/`MessageChannel` global initializer (a lazy global group).
pub(crate) fn initialize_globals(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let api = api(c)?;
    let global = c.global_root()?;
    for name in ["MessagePort", "MessageChannel"] {
        let class = get(c, api, name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, class, true, false, true)?;
    }
    Ok(c.undefined())
}

/// Register `endpoint` as a port of this VM and return its new JavaScript object.
fn adopt(c: &mut Context<'_>, endpoint: Arc<Endpoint>) -> Result<RootId, RootedError> {
    let wake = crate::modules::napi::wake_signal()
        .map_err(|error| RootedError::host(error.to_string()))?;
    endpoint.attach_wake(wake);
    let api = api(c)?;
    let id = {
        let state = state(c);
        let mut state = state.borrow_mut();
        state.messaging.next_port += 1;
        state.messaging.next_port
    };
    let create = get(c, api, "createPort")?;
    let id_value = c.number(id as f64);
    let object = c.call_rooted(create, api, &[id_value])?;
    c.require_transfer_rooted(object)?;
    let retained = c.retain(object)?;
    state(c).borrow_mut().messaging.ports.insert(
        id,
        LocalPort { endpoint, object: retained, started: false, referenced: false, closing: false },
    );
    Ok(object)
}

fn port_id(c: &mut Context<'_>, value: Option<&RootId>) -> Result<u64, RootedError> {
    let value = value.copied().ok_or_else(|| RootedError::host("message port id missing"))?;
    Ok(c.to_number(value)? as u64)
}

fn data_clone_error(c: &mut Context<'_>, message: &str) -> RootedError {
    crate::modules::clone_shared_vm::data_clone_exception(c, message)
}

/// Serialize a message with its transfer list split into buffers and ports; transferred
/// ports leave this VM.
fn serialize(c: &mut Context<'_>, value: RootId, buffers: RootId, ports: RootId, ids: RootId, source: Option<u64>) -> Result<Message, RootedError> {
    let mut buffer_roots = Vec::new();
    let length = get(c, buffers, "length")?;
    for index in 0..c.to_number(length)? as usize {
        let key = c.number(index as f64);
        buffer_roots.push(c.get_property_rooted(buffers, key)?);
    }
    let mut port_objects = Vec::new();
    let mut port_ids = Vec::new();
    let length = get(c, ports, "length")?;
    for index in 0..c.to_number(length)? as usize {
        let key = c.number(index as f64);
        port_objects.push(c.get_property_rooted(ports, key)?);
        let id = c.get_property_rooted(ids, key)?;
        port_ids.push(c.to_number(id)? as u64);
    }
    for (index, id) in port_ids.iter().enumerate() {
        if Some(*id) == source {
            return Err(data_clone_error(c, "Transfer list contains source port"));
        }
        if port_ids[..index].contains(id) {
            return Err(data_clone_error(c, "Transfer list contains duplicate MessagePort"));
        }
        let usable = state(c).borrow().messaging.ports.get(id).is_some_and(|port| !port.closing);
        if !usable {
            return Err(data_clone_error(c, "MessagePort in transfer list is already detached"));
        }
    }
    let value = c
        .serialize_rooted(value, &buffer_roots, &port_objects)
        .map_err(|error| crate::modules::clone_shared_vm::as_dom_exception(c, error))?;
    let mut endpoints = Vec::with_capacity(port_ids.len());
    for id in port_ids {
        let port = state(c).borrow_mut().messaging.ports.remove(&id).expect("validated transferred port");
        c.release_root(port.object);
        endpoints.push(port.endpoint);
    }
    Ok(Message { value, ports: endpoints })
}

/// Deserialize `message` into this VM, adopting its transferred ports. Returns the value
/// and the new port objects.
fn deserialize(c: &mut Context<'_>, message: Message) -> Result<(RootId, Vec<RootId>), RootedError> {
    let mut ports = Vec::with_capacity(message.ports.len());
    for endpoint in message.ports {
        ports.push(adopt(c, endpoint)?);
    }
    let value = c.deserialize_rooted(message.value, &ports)?;
    Ok((value, ports))
}

pub(crate) fn channel(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let (first, second) = Endpoint::channel();
    let first = adopt(c, first)?;
    let second = adopt(c, second)?;
    c.array_rooted(&[first, second])
}

pub(crate) fn post(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = port_id(c, args.first())?;
    let value = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let [buffers, ports, ids] = [2, 3, 4].map(|index| args.get(index).copied());
    let (Some(buffers), Some(ports), Some(ids)) = (buffers, ports, ids) else {
        return Err(RootedError::host("message transfer lists missing"));
    };
    let message = serialize(c, value, buffers, ports, ids, Some(id))?;
    let endpoint = state(c).borrow().messaging.ports.get(&id).filter(|port| !port.closing).map(|port| port.endpoint.clone());
    if let Some(endpoint) = endpoint {
        endpoint.send(message);
    }
    Ok(c.undefined())
}

fn with_port(c: &mut Context<'_>, args: &[RootId], update: impl FnOnce(&mut LocalPort)) -> Result<RootId, RootedError> {
    let id = port_id(c, args.first())?;
    if let Some(port) = state(c).borrow_mut().messaging.ports.get_mut(&id) {
        update(port);
    }
    Ok(c.undefined())
}

pub(crate) fn start(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    with_port(c, args, |port| port.started = true)
}

pub(crate) fn close(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    with_port(c, args, |port| {
        if !port.closing {
            port.closing = true;
            port.endpoint.close();
        }
    })
}

pub(crate) fn reference(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let referenced = match args.get(1) {
        Some(&value) => c.truthy_rooted(value)?,
        None => false,
    };
    with_port(c, args, |port| port.referenced = referenced)
}

pub(crate) fn has_ref(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = port_id(c, args.first())?;
    let referenced = state(c).borrow().messaging.ports.get(&id).is_some_and(|port| port.referenced && !port.closing);
    Ok(c.boolean(referenced))
}

/// `receiveMessageOnPort`: the next queued message as `{ message }`, or undefined.
pub(crate) fn receive(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = port_id(c, args.first())?;
    let endpoint = state(c).borrow().messaging.ports.get(&id).map(|port| port.endpoint.clone());
    let Some(message) = endpoint.and_then(|endpoint| endpoint.receive()) else {
        return Ok(c.undefined());
    };
    let (value, _) = deserialize(c, message)?;
    let result = c.object_rooted()?;
    set(c, result, "message", value)?;
    Ok(result)
}

fn find_worker(c: &mut Context<'_>, object: RootId) -> Result<Option<usize>, RootedError> {
    let workers: Vec<RootId> = state(c).borrow().messaging.workers.iter().map(|worker| worker.object).collect();
    for (index, worker) in workers.into_iter().enumerate() {
        if c.strict_equal_rooted(worker, object)? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

pub(crate) fn worker_ref(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let worker = args.first().copied().unwrap_or_else(|| c.undefined());
    let referenced = match args.get(1) {
        Some(&value) => c.truthy_rooted(value)?,
        None => false,
    };
    if let Some(index) = find_worker(c, worker)? {
        state(c).borrow_mut().messaging.workers[index].referenced = referenced;
    }
    Ok(c.undefined())
}

pub(crate) fn worker_terminate(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let worker = args.first().copied().unwrap_or_else(|| c.undefined());
    if let Some(index) = find_worker(c, worker)? {
        state(c).borrow().messaging.workers[index].control.request_termination();
    }
    Ok(c.undefined())
}

pub(crate) fn worker_exited(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let worker = args.first().copied().unwrap_or_else(|| c.undefined());
    let exited = find_worker(c, worker)?.is_none();
    Ok(c.boolean(exited))
}

/// `new Worker(...)`: start the worker thread; returns `[threadId, parentSidePort]`.
pub(crate) fn worker_start(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let argument = |index: usize| args.get(index).copied();
    let missing = || RootedError::host("worker start arguments missing");
    let worker = argument(0).ok_or_else(missing)?;
    let filename = c.to_string(argument(1).ok_or_else(missing)?)?;
    let is_eval = c.truthy_rooted(argument(2).ok_or_else(missing)?)?;
    let strings = |c: &mut Context<'_>, array: RootId| -> Result<Vec<String>, RootedError> {
        let length = get(c, array, "length")?;
        (0..c.to_number(length)? as usize)
            .map(|index| {
                let key = c.number(index as f64);
                let value = c.get_property_rooted(array, key)?;
                c.to_string(value)
            })
            .collect()
    };
    let argv = strings(c, argument(3).ok_or_else(missing)?)?;
    let exec_argv = match argument(4) {
        Some(value) if c.is_object_rooted(value)? => Some(strings(c, value)?),
        _ => None,
    };
    let env_pairs = argument(5).ok_or_else(missing)?;
    let mut env = Vec::new();
    let length = get(c, env_pairs, "length")?;
    for index in 0..c.to_number(length)? as usize {
        let key = c.number(index as f64);
        let pair = c.get_property_rooted(env_pairs, key)?;
        let pair = strings(c, pair)?;
        if let [name, value] = &pair[..] {
            env.push((name.clone(), value.clone()));
        }
    }
    let data = argument(6).unwrap_or_else(|| c.undefined());
    let [buffers, ports, ids] = [7, 8, 9].map(argument);
    let (Some(buffers), Some(ports), Some(ids)) = (buffers, ports, ids) else {
        return Err(missing());
    };
    let data = serialize(c, data, buffers, ports, ids, None)?;
    let (parent_side, worker_side) = Endpoint::channel();
    let port = adopt(c, parent_side)?;
    let (exec_path, default_exec_argv) = {
        let state = state(c);
        let state = state.borrow();
        (state.process_argv.as_slice().first().cloned().unwrap_or_default(), state.exec_argv.clone())
    };
    let launch = thread::Launch {
        filename,
        is_eval,
        argv,
        exec_path,
        exec_argv: exec_argv.unwrap_or(default_exec_argv),
        env,
        data,
        parent: worker_side,
    };
    let (thread_id, control) = thread::spawn(launch).map_err(RootedError::host)?;
    let object = c.retain(worker)?;
    state(c).borrow_mut().messaging.workers.push(LocalWorker {
        control,
        object,
        referenced: true,
        online_delivered: false,
    });
    let thread_id = c.number(thread_id as f64);
    c.array_rooted(&[thread_id, port])
}

/// The `worker_threads` exports for this VM.
pub(crate) fn module(c: &mut Context<'_>, markers: &[(&str, RootId)]) -> Result<RootId, RootedError> {
    let api = api(c)?;
    let context = c.object_rooted()?;
    for (name, marker) in markers {
        set(c, context, name, *marker)?;
    }
    let worker = state(c).borrow_mut().messaging.context.as_mut().map(|context| {
        (context.thread_id, context.parent.take(), context.data.take())
    });
    let (is_main, thread_id, parent_port, data) = match worker {
        Some((thread_id, parent, data)) => {
            let parent_port = match parent {
                Some(endpoint) => adopt(c, endpoint)?,
                None => c.null(),
            };
            let data = match data {
                Some(message) => deserialize(c, message)?.0,
                None => c.undefined(),
            };
            (false, thread_id, parent_port, data)
        }
        None => (true, 0, c.null(), c.null()),
    };
    let is_main = c.boolean(is_main);
    set(c, context, "isMainThread", is_main)?;
    let thread_id = c.number(thread_id as f64);
    set(c, context, "threadId", thread_id)?;
    set(c, context, "parentPort", parent_port)?;
    set(c, context, "workerData", data)?;
    let build = get(c, api, "module")?;
    c.call_rooted(build, api, &[context])
}

/// What the event loop delivers next.
enum Delivery {
    Message(RootId, Message),
    Close(u64, RootId),
    WorkerOnline(RootId),
    WorkerOutput(RootId, bool, Vec<u8>),
    WorkerError(RootId, thread::Failure),
    WorkerExit(usize, i32),
    Terminate,
}

fn next_delivery(state: &mut State) -> Option<Delivery> {
    if state.context.as_ref().is_some_and(|context| context.control.terminating()) {
        return Some(Delivery::Terminate);
    }
    for (id, port) in &state.ports {
        if port.started && !port.closing {
            if let Some(message) = port.endpoint.receive() {
                return Some(Delivery::Message(port.object, message));
            }
        }
        if port.closing || port.endpoint.peer_closed() {
            return Some(Delivery::Close(*id, port.object));
        }
    }
    for (index, worker) in state.workers.iter_mut().enumerate() {
        if !worker.online_delivered && worker.control.is_online() {
            worker.online_delivered = true;
            return Some(Delivery::WorkerOnline(worker.object));
        }
        if let Some((stderr, bytes)) = worker.control.take_output() {
            return Some(Delivery::WorkerOutput(worker.object, stderr, bytes));
        }
        if let Some(failure) = worker.control.take_error() {
            return Some(Delivery::WorkerError(worker.object, failure));
        }
        if let Some(code) = worker.control.take_exit() {
            return Some(Delivery::WorkerExit(index, code));
        }
    }
    None
}

/// `messagingDrain`: deliver one event; `true` when one ran.
pub(crate) fn drain(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let delivery = next_delivery(&mut state(c).borrow_mut().messaging);
    let Some(delivery) = delivery else {
        return Ok(c.boolean(false));
    };
    let api = api(c)?;
    let undefined = c.undefined();
    match delivery {
        Delivery::Message(port, message) => {
            let (value, ports) = deserialize(c, message)?;
            let ports = c.array_rooted(&ports)?;
            let dispatch = get(c, api, "dispatch")?;
            c.call_rooted(dispatch, undefined, &[port, value, ports])?;
        }
        Delivery::Close(id, port) => {
            let removed = state(c).borrow_mut().messaging.ports.remove(&id);
            if let Some(removed) = removed {
                removed.endpoint.close();
                let closed = get(c, api, "closed")?;
                let result = c.call_rooted(closed, undefined, &[port]);
                c.release_root(removed.object);
                result?;
            }
        }
        Delivery::WorkerOnline(worker) => {
            let event = get(c, api, "workerEvent")?;
            let kind = c.string_rooted("online");
            c.call_rooted(event, undefined, &[worker, kind])?;
        }
        Delivery::WorkerOutput(worker, stderr, bytes) => {
            let event = get(c, api, "workerEvent")?;
            let kind = c.string_rooted(if stderr { "stderr" } else { "stdout" });
            let chunk = c.uint8_array_rooted(&bytes)?;
            c.call_rooted(event, undefined, &[worker, kind, chunk])?;
        }
        Delivery::WorkerError(worker, failure) => {
            let error = match failure {
                thread::Failure::Thrown(value) => c.deserialize_rooted(value, &[])?,
                thread::Failure::Message(message) => c.error_rooted(&message)?,
            };
            let event = get(c, api, "workerEvent")?;
            let kind = c.string_rooted("error");
            c.call_rooted(event, undefined, &[worker, kind, error])?;
        }
        Delivery::WorkerExit(index, code) => {
            let worker = state(c).borrow_mut().messaging.workers.remove(index);
            let event = get(c, api, "workerEvent")?;
            let kind = c.string_rooted("exit");
            let code = c.number(f64::from(code));
            let result = c.call_rooted(event, undefined, &[worker.object, kind, code]);
            c.release_root(worker.object);
            result?;
        }
        Delivery::Terminate => {
            c.host_mut().shared_state().borrow().process_control.request_really_exit(thread::TERMINATED_EXIT_CODE);
            return Err(RootedError::host("process.exit"));
        }
    }
    Ok(c.boolean(true))
}

pub(crate) fn has_work(shared_state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>) -> bool {
    shared_state.borrow().messaging.has_work()
}

/// Event-loop source: deliver pending messaging events; `true` when any ran.
pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
) -> Result<bool, String> {
    if !has_work(shared_state) {
        return Ok(false);
    }
    crate::modules::process_shared_vm::deliver_each(runtime, program, "messagingDrain")
}

impl State {
    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        let Self { ports, workers, next_port, api, context } = self;
        require_quiescent(ports.is_empty() && workers.is_empty() && context.is_none(), "a message port")?;
        next_port.write(writer)?;
        api.write(writer)
    }
    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        use crate::host::startup_snapshot::HostSnap;
        self.next_port = HostSnap::read(reader)?;
        self.api = HostSnap::read(reader)?;
        Ok(())
    }
}
