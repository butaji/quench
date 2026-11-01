//! `worker_threads` and the global `MessageChannel`/`MessagePort`/`BroadcastChannel`.
//!
//! A channel is two process-wide endpoints (`ports`); each VM keeps the ports it holds in
//! its `State`, and a transferred port's endpoint moves with the message that carries it.
//! A broadcast channel is a lone endpoint subscribed under its name: a message posted to
//! one is delivered to every other endpoint of that name in the process. Messages are
//! structured-serialized (`quench_runtime::SerializedValue`), so they cross between heaps
//! on different threads. A `Worker` runs a complete Node environment on its own thread
//! (`thread`). The event loop delivers port messages, port closure and worker lifecycle
//! events through `messagingDrain`, one event per call. The JavaScript classes are `api`'s.

mod api;
mod ports;
mod thread;

#[cfg(feature = "native-execution")]
pub(crate) const NATIVE_ENTRY_UNAVAILABLE: &str = "native worker entry is not linked into this image";

pub(crate) use api::{
    broadcast_close, broadcast_constructor, broadcast_name, broadcast_post_message, broadcast_ref, broadcast_unref,
    channel_constructor, get_environment_data, handler_get, handler_set, message_event_constructor, message_event_field,
    move_message_port_to_context, port_close, port_constructor, port_has_ref, port_listener_hook, port_post_message,
    port_ref, port_start, port_unref, post_message_to_thread, receive_message_on_port, set_environment_data,
    worker_constructor, worker_forward, worker_getter, worker_not_running, worker_post_message, worker_ref,
    worker_resolve_exit, worker_resource_has_ref, worker_terminate, worker_terminate_callback, worker_unref,
};
use crate::host::NodeHost;
use ports::{Endpoint, Message};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, Weak};
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
    /// The name a broadcast channel's endpoint is subscribed under.
    broadcast: Option<String>,
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
    /// The classes `api::build` makes, built once per VM.
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

/// Every broadcast channel endpoint in the process, by channel name.
static BROADCAST_CHANNELS: Mutex<Option<HashMap<String, Vec<Weak<Endpoint>>>>> = Mutex::new(None);

fn state(c: &mut Context<'_>) -> std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>> {
    c.host_mut().shared_state()
}

/// The messaging API object, built on first use.
fn api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(api) = state(c).borrow().messaging.api {
        return Ok(api);
    }
    let api = api::build(c)?;
    let api = c.retain(api)?;
    state(c).borrow_mut().messaging.api = Some(api);
    Ok(api)
}

/// `MessagePort`/`MessageChannel`/`BroadcastChannel` global initializer (a lazy global group).
pub(crate) fn initialize_globals(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    for name in crate::modules::events_shared_vm::MESSAGING_GLOBALS {
        let class = api::global_class(c, name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, class, true, false, true)?;
    }
    Ok(c.undefined())
}

/// Register `endpoint` as a port of this VM and return its new JavaScript object.
fn adopt(c: &mut Context<'_>, endpoint: Arc<Endpoint>) -> Result<RootId, RootedError> {
    let id = register(c, endpoint, None)?;
    let object = api::create_port(c, id)?;
    c.require_transfer_rooted(object)?;
    let retained = c.retain(object)?;
    if let Some(port) = state(c).borrow_mut().messaging.ports.get_mut(&id) {
        port.object = retained;
    }
    Ok(object)
}

/// Add `endpoint` to this VM's port table under a new id, waking this event loop on its work.
fn register(c: &mut Context<'_>, endpoint: Arc<Endpoint>, broadcast: Option<String>) -> Result<u64, RootedError> {
    let wake = crate::modules::napi::wake_signal().map_err(|error| RootedError::host(error.to_string()))?;
    endpoint.attach_wake(wake);
    let placeholder = c.undefined();
    let shared = state(c);
    let mut shared = shared.borrow_mut();
    shared.messaging.next_port += 1;
    let id = shared.messaging.next_port;
    let referenced = broadcast.is_some();
    let port = LocalPort { endpoint, object: placeholder, started: referenced, referenced, closing: false, broadcast };
    shared.messaging.ports.insert(id, port);
    Ok(id)
}

fn with_port(c: &mut Context<'_>, id: u64, update: impl FnOnce(&mut LocalPort)) {
    if let Some(port) = state(c).borrow_mut().messaging.ports.get_mut(&id) {
        update(port);
    }
}

fn data_clone_error(c: &mut Context<'_>, message: &str) -> RootedError {
    crate::modules::clone_shared_vm::data_clone_exception(c, message)
}

/// Serialize a message with its transfers; transferred ports leave this VM. `source` is
/// the sending port, which may not transfer itself.
fn serialize(c: &mut Context<'_>, value: RootId, transfers: api::Transfers, source: Option<u64>) -> Result<Message, RootedError> {
    let api::Transfers { buffers, ports } = transfers;
    for (index, (_, id)) in ports.iter().enumerate() {
        if Some(*id) == source {
            return Err(data_clone_error(c, "Transfer list contains source port"));
        }
        if ports[..index].iter().any(|(_, other)| other == id) {
            return Err(data_clone_error(c, "Transfer list contains duplicate MessagePort"));
        }
        let usable = state(c).borrow().messaging.ports.get(id).is_some_and(|port| !port.closing);
        if !usable {
            return Err(data_clone_error(c, "MessagePort in transfer list is already detached"));
        }
    }
    let objects: Vec<RootId> = ports.iter().map(|(object, _)| *object).collect();
    let value = c
        .serialize_rooted(value, &buffers, &objects)
        .map_err(|error| crate::modules::clone_shared_vm::as_dom_exception(c, error))?;
    let mut endpoints = Vec::with_capacity(ports.len());
    for (_, id) in ports {
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

/// Two entangled ports of this VM.
fn channel(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let (first, second) = Endpoint::channel();
    Ok((adopt(c, first)?, adopt(c, second)?))
}

/// Send `value` from port `id` to its peer.
fn post(c: &mut Context<'_>, id: u64, value: RootId, transfers: api::Transfers) -> Result<(), RootedError> {
    let message = serialize(c, value, transfers, Some(id))?;
    let endpoint = state(c).borrow().messaging.ports.get(&id).filter(|port| !port.closing).map(|port| port.endpoint.clone());
    if let Some(endpoint) = endpoint {
        endpoint.send(message);
    }
    Ok(())
}

/// `receiveMessageOnPort`: the next queued message as `{ message }`, or undefined.
fn receive(c: &mut Context<'_>, id: u64) -> Result<RootId, RootedError> {
    let endpoint = state(c).borrow().messaging.ports.get(&id).map(|port| port.endpoint.clone());
    let Some(message) = endpoint.and_then(|endpoint| endpoint.receive()) else {
        return Ok(c.undefined());
    };
    let (value, _) = deserialize(c, message)?;
    let result = c.object_rooted()?;
    c.assign_named_property_rooted(result, "message", value)?;
    Ok(result)
}

fn broadcast_channels<T>(update: impl FnOnce(&mut HashMap<String, Vec<Weak<Endpoint>>>) -> T) -> T {
    let mut channels = BROADCAST_CHANNELS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    update(channels.get_or_insert_with(HashMap::new))
}

/// Subscribe the broadcast channel `object` to `name`; returns its port id.
fn subscribe(c: &mut Context<'_>, object: RootId, name: &str) -> Result<u64, RootedError> {
    let endpoint = Endpoint::lone();
    broadcast_channels(|channels| channels.entry(name.to_owned()).or_default().push(Arc::downgrade(&endpoint)));
    let id = register(c, endpoint, Some(name.to_owned()))?;
    let retained = c.retain(object)?;
    with_port(c, id, |port| port.object = retained);
    Ok(id)
}

/// Close a broadcast channel's endpoint: it receives nothing more and leaves this VM.
fn unsubscribe(c: &mut Context<'_>, id: u64) {
    let removed = state(c).borrow_mut().messaging.ports.remove(&id);
    if let Some(port) = removed {
        port.endpoint.close();
        c.release_root(port.object);
        if let Some(name) = port.broadcast {
            broadcast_channels(|channels| {
                if let Some(subscribers) = channels.get_mut(&name) {
                    subscribers.retain(|subscriber| subscriber.upgrade().is_some_and(|endpoint| !endpoint.is_closed()));
                }
            });
        }
    }
}

/// Deliver `value` to every other open channel subscribed under the sender's name.
fn broadcast(c: &mut Context<'_>, id: u64, value: RootId) -> Result<(), RootedError> {
    let sender = state(c).borrow().messaging.ports.get(&id).map(|port| (port.endpoint.clone(), port.broadcast.clone()));
    let Some((sender, Some(name))) = sender else { return Ok(()) };
    let receivers: Vec<Arc<Endpoint>> = broadcast_channels(|channels| {
        channels
            .get(&name)
            .into_iter()
            .flatten()
            .filter_map(Weak::upgrade)
            .filter(|endpoint| !Arc::ptr_eq(endpoint, &sender) && !endpoint.is_closed())
            .collect()
    });
    for receiver in receivers {
        let message = serialize(c, value, api::Transfers { buffers: Vec::new(), ports: Vec::new() }, None)?;
        receiver.deliver(message);
    }
    Ok(())
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

fn set_worker_referenced(c: &mut Context<'_>, worker: RootId, referenced: bool) -> Result<(), RootedError> {
    if let Some(index) = find_worker(c, worker)? {
        state(c).borrow_mut().messaging.workers[index].referenced = referenced;
    }
    Ok(())
}

fn terminate_worker(c: &mut Context<'_>, worker: RootId) -> Result<(), RootedError> {
    if let Some(index) = find_worker(c, worker)? {
        state(c).borrow().messaging.workers[index].control.request_termination();
    }
    Ok(())
}

fn worker_exited(c: &mut Context<'_>, worker: RootId) -> Result<bool, RootedError> {
    Ok(find_worker(c, worker)?.is_none())
}

/// What `new Worker(...)` starts a thread with.
struct WorkerStart {
    entry: String,
    is_eval: bool,
    argv: Vec<String>,
    exec_argv: Option<Vec<String>>,
    env: Vec<(String, String)>,
    data: RootId,
    transfers: api::Transfers,
    environment: RootId,
}

/// Start `worker`'s thread; returns its thread id and parent-side port.
fn start_worker(c: &mut Context<'_>, worker: RootId, start: WorkerStart) -> Result<(u64, RootId), RootedError> {
    let WorkerStart { entry, is_eval, argv, exec_argv, env, data, transfers, environment } = start;
    let data = serialize(c, data, transfers, None)?;
    let environment = serialize(c, environment, api::Transfers { buffers: Vec::new(), ports: Vec::new() }, None)?;
    let (parent_side, worker_side) = Endpoint::channel();
    let port = adopt(c, parent_side)?;
    let (exec_path, default_exec_argv) = {
        let state = state(c);
        let state = state.borrow();
        (state.process_argv.as_slice().first().cloned().unwrap_or_default(), state.exec_argv.clone())
    };
    let launch = thread::Launch {
        filename: entry,
        is_eval,
        argv,
        exec_path,
        exec_argv: exec_argv.unwrap_or(default_exec_argv),
        env,
        data,
        environment,
        parent: worker_side,
    };
    let (thread_id, control) = thread::spawn(launch).map_err(RootedError::host)?;
    let object = c.retain(worker)?;
    state(c).borrow_mut().messaging.workers.push(LocalWorker { control, object, referenced: true, online_delivered: false });
    Ok((thread_id, port))
}

/// The `worker_threads` exports for this VM.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let worker = state(c).borrow_mut().messaging.context.as_mut().map(|context| {
        (context.thread_id, context.parent.take(), context.data.take(), context.environment.take())
    });
    let facts = match worker {
        Some((thread_id, parent, data, environment)) => {
            let parent_port = match parent {
                Some(endpoint) => adopt(c, endpoint)?,
                None => c.null(),
            };
            let data = match data {
                Some(message) => deserialize(c, message)?.0,
                None => c.undefined(),
            };
            if let Some(environment) = environment {
                let inherited = deserialize(c, environment)?.0;
                api::adopt_environment(c, inherited)?;
            }
            api::ThreadFacts { is_main: false, thread_id, parent_port, data }
        }
        None => {
            let null = c.null();
            api::ThreadFacts { is_main: true, thread_id: 0, parent_port: null, data: null }
        }
    };
    api::module(c, facts)
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
    match delivery {
        Delivery::Message(port, message) => {
            let (value, ports) = deserialize(c, message)?;
            let ports = c.array_rooted(&ports)?;
            api::dispatch_message(c, port, value, ports)?;
        }
        Delivery::Close(id, port) => {
            let removed = state(c).borrow_mut().messaging.ports.remove(&id);
            if let Some(removed) = removed {
                removed.endpoint.close();
                let result = api::dispatch_close(c, port);
                c.release_root(removed.object);
                result?;
            }
        }
        Delivery::WorkerOnline(worker) => api::worker_event(c, worker, api::WorkerEvent::Online)?,
        Delivery::WorkerOutput(worker, stderr, bytes) => {
            let chunk = crate::modules::buffer_shared_vm::from_vec(c, bytes)?;
            api::worker_event(c, worker, api::WorkerEvent::Output { stderr, chunk })?;
        }
        Delivery::WorkerError(worker, failure) => {
            let error = match failure {
                thread::Failure::Thrown(value) => c.deserialize_rooted(value, &[])?,
                thread::Failure::Message(message) => c.error_rooted(&message)?,
            };
            api::worker_event(c, worker, api::WorkerEvent::Error(error))?;
        }
        Delivery::WorkerExit(index, code) => {
            let worker = state(c).borrow_mut().messaging.workers.remove(index);
            let result = api::worker_event(c, worker.object, api::WorkerEvent::Exit(code));
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
