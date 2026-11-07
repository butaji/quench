//! The JavaScript surface of `worker_threads` and the global `MessagePort`, `MessageChannel`
//! and `BroadcastChannel` (lib/internal/worker.js, lib/internal/worker/io.js), over the port
//! transport and worker threads of the parent module.
//!
//! A port is a `NodeEventTarget` whose private slot holds its id in this VM's port table;
//! its listener hook refs and starts it while it has 'message' listeners. A worker is an
//! `EventEmitter` whose private slot holds its record: thread id, parent-side port and
//! stdio streams.
use super::{state, Context};
use crate::modules::event_target_shared_vm as events;
use crate::modules::stream_shared_vm::call_method;
use crate::modules::util_shared_vm::{argument_type_error, argument_type_error_with_expected};
use quench_runtime::{DescriptorFields, RootId, RootedError, ValueType};

/// The members of the per-VM API object.
mod member {
    pub(super) const MESSAGE_PORT: &str = "MessagePort";
    pub(super) const MESSAGE_CHANNEL: &str = "MessageChannel";
    pub(super) const BROADCAST_CHANNEL: &str = "BroadcastChannel";
    pub(super) const WORKER: &str = "Worker";
    pub(super) const MESSAGE_EVENT: &str = "MessageEvent";
    pub(crate) use crate::modules::keys::SLOT;
    /// The private symbol an event's MessageEvent fields live under.
    pub(super) const EVENT_FIELDS: &str = "eventFields";
    /// `setEnvironmentData`'s map.
    pub(super) const ENVIRONMENT: &str = "environment";
    /// The ports' shared listener hook.
    pub(super) const PORT_HOOK: &str = "portHook";
}

/// A worker record's fields.
mod worker {
    pub(super) const THREAD_ID: &str = "threadId";
    pub(crate) use crate::modules::keys::PORT;
    pub(crate) use crate::modules::keys::STDOUT;
    pub(crate) use crate::modules::keys::STDERR;
    pub(super) const REFERENCED: &str = "referenced";
    pub(super) const EXIT: &str = "exit";
}

/// A broadcast channel record's fields.
mod broadcast {
    pub(crate) use crate::modules::keys::NAME;
    pub(crate) use crate::modules::keys::PORT;
}

/// The `MessageEvent` fields `new MessageEvent(type, init)` reads, in its getters' order.
const MESSAGE_EVENT_FIELDS: [&str; 5] = ["data", "origin", "lastEventId", "source", "ports"];
/// An exited worker's `threadId`.
const EXITED_THREAD_ID: f64 = -1.0;
const MAIN_THREAD_NAME: &str = "main";
const WORKER_THREAD_NAME: &str = "WorkerThread";
const SHARE_ENV_KEY: &str = "nodejs.worker_threads.SHARE_ENV";
/// lib/internal/worker.js: a worker filename that is neither absolute nor `./`/`../` relative.
const RELATIVE_PREFIXES: [&str; 4] = ["./", "../", ".\\", "..\\"];
const WORKER_RESOURCE: &str = "WORKER";
const PORT_RESOURCE: &str = "MESSAGEPORT";
const MESSAGE_EVENT_TYPE: &str = "message";
const BROADCAST_CLOSED: &str = "BroadcastChannel is closed.";
const INVALID_STATE_ERROR: &str = "InvalidStateError";

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn kind(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    kind(c, value) == ValueType::Undefined
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    matches!(kind(c, value), ValueType::Undefined | ValueType::Null)
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}

fn operation(name: &str) -> quench_runtime::HostFunctionId {
    crate::host::shared_vm::operation(name)
}

fn coded(c: &mut Context<'_>, type_error: bool, code: &'static str, message: &str) -> RootedError {
    let error = if type_error { c.type_error_rooted(message) } else { c.error_rooted(message) };
    let error = error.and_then(|error| {
        if type_error {
            crate::modules::node_errors_shared_vm::decorate_type_error(c, error, code)?;
        } else {
            crate::modules::node_errors_shared_vm::decorate_error(c, error, code)?;
        }
        Ok(error)
    });
    match error {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// Methods and accessors on `prototype`: `(name, operation, data)` methods are enumerable
/// as V8 templates make them; accessors as `defineEventHandler` does.
fn methods(c: &mut Context<'_>, prototype: RootId, list: &[(&str, &str)], enumerable: bool) -> Result<(), RootedError> {
    for (name, op) in list {
        let function = c.host_function_named(operation(op), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, enumerable, true)?;
    }
    Ok(())
}

fn accessor(c: &mut Context<'_>, holder: RootId, name: &str, getter: &str, setter: Option<&str>, data: RootId) -> Result<(), RootedError> {
    let get = c.host_function_named_with_data(operation(getter), &format!("get {name}"), data)?;
    let set = match setter {
        Some(setter) => c.host_function_named_with_data(operation(setter), &format!("set {name}"), data)?,
        None => c.undefined(),
    };
    let key = c.string_rooted(name);
    let fields = DescriptorFields { get: Some(get), set: Some(set), enumerable: Some(true), configurable: Some(true), ..Default::default() };
    c.define_property_fields_rooted(holder, key, fields)?;
    Ok(())
}

/// `class Sub extends Base`: both chains, and the read-only `prototype` a class has.
fn extend(c: &mut Context<'_>, class: RootId, base: RootId) -> Result<RootId, RootedError> {
    c.set_prototype_rooted(class, base)?;
    let prototype = get(c, class, "prototype")?;
    let base_prototype = get(c, base, "prototype")?;
    c.set_prototype_rooted(prototype, base_prototype)?;
    c.define_named_data_property_rooted(class, "prototype", prototype, false, false, false)?;
    Ok(prototype)
}

/// Build this VM's messaging classes.
pub(super) fn build(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let api = c.null_object_rooted()?;
    for name in [member::SLOT, member::EVENT_FIELDS] {
        let symbol = c.private_symbol_rooted(Some(name));
        put(c, api, name, symbol)?;
    }
    let global = c.global_root()?;
    let map = get(c, global, "Map")?;
    let environment = c.construct_rooted(map, map, &[])?;
    put(c, api, member::ENVIRONMENT, environment)?;
    let hook = c.host_function_named(crate::host::shared_vm::operation!("messagingPortListenerHook"), "kNewListener")?;
    put(c, api, member::PORT_HOOK, hook)?;

    let node_target = events::node_event_target(c)?;
    let port = c.host_constructor_named(crate::host::shared_vm::operation!("messagingPortConstructor"), member::MESSAGE_PORT)?;
    let prototype = extend(c, port, node_target)?;
    let key = c.string_rooted("constructor");
    c.delete_property_rooted(prototype, key)?;
    methods(c, prototype, &[("postMessage", "messagingPortPostMessage"), ("start", "messagingPortStart")], true)?;
    c.define_data_property_rooted(prototype, key, port, true, false, true)?;
    methods(c, prototype, &[("ref", "messagingPortRef"), ("unref", "messagingPortUnref"), ("hasRef", "messagingPortHasRef")], true)?;
    for kind in ["message", "messageerror"] {
        let data = c.string_rooted(kind);
        accessor(c, prototype, &format!("on{kind}"), "messagingHandlerGet", Some("messagingHandlerSet"), data)?;
    }
    methods(c, prototype, &[("close", "messagingPortClose")], true)?;
    put(c, api, member::MESSAGE_PORT, port)?;

    let channel = c.host_constructor_named(crate::host::shared_vm::operation!("messagingChannelConstructor"), member::MESSAGE_CHANNEL)?;
    put(c, api, member::MESSAGE_CHANNEL, channel)?;

    let data = events_api(c)?;
    let event = get(c, data, "eventCtor")?;
    let target = get(c, data, "eventTargetCtor")?;
    let message_event = c.host_constructor_named(crate::host::shared_vm::operation!("messagingMessageEventConstructor"), member::MESSAGE_EVENT)?;
    let prototype = extend(c, message_event, event)?;
    for (index, name) in MESSAGE_EVENT_FIELDS.iter().enumerate() {
        let data = c.number(index as f64);
        accessor(c, prototype, name, "messagingMessageEventField", None, data)?;
    }
    put(c, api, member::MESSAGE_EVENT, message_event)?;

    let channel = c.host_constructor_named(crate::host::shared_vm::operation!("messagingBroadcastConstructor"), member::BROADCAST_CHANNEL)?;
    let prototype = extend(c, channel, target)?;
    let undefined = c.undefined();
    accessor(c, prototype, "name", "messagingBroadcastName", None, undefined)?;
    methods(
        c,
        prototype,
        &[
            ("close", "messagingBroadcastClose"),
            ("postMessage", "messagingBroadcastPostMessage"),
            ("ref", "messagingBroadcastRef"),
            ("unref", "messagingBroadcastUnref"),
        ],
        false,
    )?;
    for kind in ["message", "messageerror"] {
        let data = c.string_rooted(kind);
        accessor(c, prototype, &format!("on{kind}"), "messagingHandlerGet", Some("messagingHandlerSet"), data)?;
    }
    let symbol = get(c, global, "Symbol")?;
    let registry = get(c, symbol, "for")?;
    let description = c.string_rooted(INSPECT_CUSTOM);
    let inspect_key = c.call_rooted(registry, symbol, &[description])?;
    let inspect = c.host_function_named(crate::host::shared_vm::operation!("messagingBroadcastInspect"), &format!("[{INSPECT_CUSTOM}]"))?;
    c.define_data_property_rooted(prototype, inspect_key, inspect, true, false, true)?;
    put(c, api, member::BROADCAST_CHANNEL, channel)?;

    let emitter = crate::host::shared_vm::commonjs::events_module(c)?;
    let worker_class = c.host_constructor_named(crate::host::shared_vm::operation!("workerConstructor"), member::WORKER)?;
    let prototype = extend(c, worker_class, emitter)?;
    methods(
        c,
        prototype,
        &[
            ("postMessage", "workerPostMessage"),
            ("terminate", "workerThreadTerminate"),
            ("ref", "workerThreadRef"),
            ("unref", "workerThreadUnref"),
        ],
        false,
    )?;
    for (index, name) in WORKER_GETTERS.iter().enumerate() {
        let data = c.number(index as f64);
        let get = c.host_function_named_with_data(crate::host::shared_vm::operation!("workerGetter"), &format!("get {name}"), data)?;
        let key = c.string_rooted(name);
        let undefined = c.undefined();
        let fields = DescriptorFields { get: Some(get), set: Some(undefined), enumerable: Some(false), configurable: Some(true), ..Default::default() };
        c.define_property_fields_rooted(prototype, key, fields)?;
    }
    for name in ["getHeapSnapshot", "getHeapStatistics", "cpuUsage", "startCpuProfile"] {
        let function = c.host_function_named(crate::host::shared_vm::operation!("workerNotRunning"), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    put(c, api, member::WORKER, worker_class)?;
    Ok(api)
}

/// The DOM event family's data, built on first use.
fn events_api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::modules::events_shared_vm::event_family(c)
}

fn member(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let api = super::api(c)?;
    get(c, api, name)
}

/// The value `object` keeps in the API's private slot, if any.
fn slot(c: &mut Context<'_>, object: RootId) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(None);
    }
    let key = member(c, member::SLOT)?;
    let value = c.get_property_rooted(object, key)?;
    Ok((!is_undefined(c, value)).then_some(value))
}

fn set_slot(c: &mut Context<'_>, object: RootId, value: RootId) -> Result<(), RootedError> {
    let key = member(c, member::SLOT)?;
    c.define_data_property_rooted(object, key, value, false, false, false)?;
    Ok(())
}

/// The port id of `value`, when it is a MessagePort of this VM.
pub(super) fn port_id(c: &mut Context<'_>, value: RootId) -> Result<Option<u64>, RootedError> {
    let port = member(c, member::MESSAGE_PORT)?;
    if !c.is_object_rooted(value)? || !c.instance_of_rooted(value, port)? {
        return Ok(None);
    }
    let id = slot(c, value)?;
    Ok(id.and_then(|id| c.rooted_value(id)).and_then(|id| id.as_number()).map(|id| id as u64))
}

fn receiver_port(c: &mut Context<'_>, receiver: RootId) -> Result<u64, RootedError> {
    match port_id(c, receiver)? {
        Some(id) => Ok(id),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            Err(c.throw(error))
        }
    }
}

/// A new MessagePort object for port `id`.
pub(super) fn create_port(c: &mut Context<'_>, id: u64) -> Result<RootId, RootedError> {
    let port = member(c, member::MESSAGE_PORT)?;
    let prototype = get(c, port, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    events::initialize_node_target(c, object)?;
    let id = c.number(id as f64);
    set_slot(c, object, id)?;
    let hook = member(c, member::PORT_HOOK)?;
    let data = events_api(c)?;
    let key = get(c, data, events::LISTENER_HOOK_SLOT)?;
    c.define_data_property_rooted(object, key, hook, true, false, true)?;
    emit_init(c, object, PORT_RESOURCE)?;
    Ok(object)
}

/// `async_hooks` init for a host resource.
fn emit_init(c: &mut Context<'_>, resource: RootId, kind: &str) -> Result<(), RootedError> {
    let shared = c.host_mut().shared_state();
    let id = shared.borrow().async_hooks.identity.allocate_async_id();
    crate::modules::async_hooks_shared_vm::emit_init(c, resource, kind, id)
}

/// `new MessagePort()`: ports are made only by channels and transfers.
pub(crate) fn port_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Err(coded(c, true, "ERR_CONSTRUCT_CALL_INVALID", "Constructor cannot be called"))
}

/// The transfer list of `postMessage(value, transferList)` or of a Worker's options: an
/// iterable, or an object whose `transfer` is one.
fn transfer_list(c: &mut Context<'_>, list: RootId, unwrap_options: bool) -> Result<Vec<RootId>, RootedError> {
    if is_undefined(c, list) {
        return Ok(Vec::new());
    }
    let mut list = list;
    if unwrap_options && !is_iterable(c, list)? && c.is_object_rooted(list)? {
        let transfer = get(c, list, "transfer")?;
        if !is_undefined(c, transfer) {
            list = transfer;
        }
    }
    if !is_iterable(c, list)? {
        return Err(coded(c, true, "ERR_INVALID_ARG_TYPE", "Optional transferList argument must be an iterable"));
    }
    let iterator = c.iterator_rooted(list)?;
    let mut items = Vec::new();
    while let Some(item) = c.iterator_step_rooted(&iterator)? {
        items.push(item);
    }
    Ok(items)
}

fn is_iterable(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, value) {
        return Ok(false);
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator = get(c, symbol, "iterator")?;
    let method = c.get_property_rooted(value, iterator)?;
    c.is_callable_rooted(method)
}

/// A transfer list split into transferred ports (with their ids) and other transferables.
pub(super) struct Transfers {
    pub(super) buffers: Vec<RootId>,
    pub(super) ports: Vec<(RootId, u64)>,
}

fn split_transfers(c: &mut Context<'_>, items: Vec<RootId>) -> Result<Transfers, RootedError> {
    let mut transfers = Transfers { buffers: Vec::new(), ports: Vec::new() };
    for item in items {
        match port_id(c, item)? {
            Some(id) => transfers.ports.push((item, id)),
            None => transfers.buffers.push(item),
        }
    }
    Ok(transfers)
}

/// `port.postMessage(value, transferList)`.
pub(crate) fn port_post_message(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver_port(c, receiver)?;
    let value = argument(c, args, 0);
    let list = argument(c, args, 1);
    let items = transfer_list(c, list, true)?;
    let transfers = split_transfers(c, items)?;
    super::post(c, id, value, transfers)?;
    Ok(c.undefined())
}

pub(crate) fn port_start(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver_port(c, receiver)?;
    super::with_port(c, id, |port| port.started = true);
    Ok(c.undefined())
}

pub(crate) fn port_close(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver_port(c, receiver)?;
    super::with_port(c, id, |port| {
        if !port.closing {
            port.closing = true;
            port.endpoint.close();
        }
    });
    Ok(c.undefined())
}

fn set_referenced(c: &mut Context<'_>, receiver: RootId, referenced: bool) -> Result<RootId, RootedError> {
    let id = receiver_port(c, receiver)?;
    super::with_port(c, id, |port| port.referenced = referenced);
    Ok(c.undefined())
}

pub(crate) fn port_ref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    set_referenced(c, receiver, true)
}

pub(crate) fn port_unref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    set_referenced(c, receiver, false)
}

pub(crate) fn port_has_ref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = receiver_port(c, receiver)?;
    let referenced = state(c).borrow().messaging.ports.get(&id).is_some_and(|port| port.referenced && !port.closing);
    Ok(c.boolean(referenced))
}

/// `setupPortReferencing`: a port with 'message' listeners is referenced and started; one
/// whose last listener left is unreferenced.
pub(crate) fn port_listener_hook(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let kind = argument(c, args, 0);
    if c.string_text(kind)?.as_deref() != Some(MESSAGE_EVENT_TYPE) {
        return Ok(c.undefined());
    }
    let size = argument(c, args, 1);
    let size = c.to_number(size)?;
    let Some(id) = port_id(c, receiver)?.or(broadcast_port(c, receiver)?) else {
        return Ok(c.undefined());
    };
    super::with_port(c, id, |port| {
        port.referenced = size > 0.0;
        port.started |= size > 0.0;
    });
    Ok(c.undefined())
}

/// The handler table an `on<event>` attribute stores its handlers in.
fn handlers(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    let key = member(c, member::EVENT_FIELDS)?;
    let table = c.get_property_rooted(receiver, key)?;
    if !is_undefined(c, table) {
        return Ok(table);
    }
    let table = c.null_object_rooted()?;
    c.define_data_property_rooted(receiver, key, table, false, false, false)?;
    Ok(table)
}

/// `get on<event>`: the handler, or null.
pub(crate) fn handler_get(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let kind = c.host_function_data()?;
    let kind = c.to_string(kind)?;
    let table = handlers(c, receiver)?;
    let holder = get(c, table, &kind)?;
    if is_undefined(c, holder) {
        return Ok(c.null());
    }
    let handler = get(c, holder, "handler")?;
    Ok(if c.is_callable_rooted(handler)? { handler } else { c.null() })
}

/// `set on<event>(handler)` (`defineEventHandler`): the first function registers a listener
/// that calls the current handler; later assignments only replace the handler.
pub(crate) fn handler_set(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let kind = c.host_function_data()?;
    let kind_text = c.to_string(kind)?;
    let value = argument(c, args, 0);
    let table = handlers(c, receiver)?;
    let holder = get(c, table, &kind_text)?;
    if !is_undefined(c, holder) {
        put(c, holder, "handler", value)?;
        return Ok(c.undefined());
    }
    if !c.is_callable_rooted(value)? {
        return Ok(c.undefined());
    }
    let holder = c.null_object_rooted()?;
    put(c, holder, "handler", value)?;
    put(c, table, &kind_text, holder)?;
    let wrapper = c.host_function_named_with_data(crate::host::shared_vm::operation!("eventHandlerInvoke"), "eventHandler", holder)?;
    let add = get(c, receiver, "addEventListener")?;
    c.call_rooted(add, receiver, &[kind, wrapper])?;
    Ok(c.undefined())
}

/// `new MessageChannel()`: two entangled ports as `port1` and `port2`.
pub(crate) fn channel_constructor(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    events::require_new(c, member::MESSAGE_CHANNEL)?;
    let (first, second) = super::channel(c)?;
    for (name, port) in [("port1", first), ("port2", second)] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(receiver, key, port, true, true, true)?;
    }
    Ok(receiver)
}

/// `new MessageEvent(type, { data, origin, lastEventId, source, ports })`.
pub(crate) fn message_event_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let kind = argument(c, args, 0);
    events::event_constructor(c, receiver, &[kind])?;
    let init = argument(c, args, 1);
    let fields = c.null_object_rooted()?;
    for name in MESSAGE_EVENT_FIELDS {
        let value = if c.is_object_rooted(init)? { get(c, init, name)? } else { c.undefined() };
        let value = match (name, is_undefined(c, value)) {
            ("data" | "source", true) => c.null(),
            ("origin" | "lastEventId", true) => c.string_rooted(""),
            ("ports", true) => c.array_rooted(&[])?,
            _ => value,
        };
        put(c, fields, name, value)?;
    }
    let key = member(c, member::EVENT_FIELDS)?;
    c.define_data_property_rooted(receiver, key, fields, false, false, false)?;
    Ok(receiver)
}

pub(crate) fn message_event_field(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let index = c.host_function_data()?;
    let index = c.to_number(index)? as usize;
    let key = member(c, member::EVENT_FIELDS)?;
    let fields = if c.is_object_rooted(receiver)? { c.get_property_rooted(receiver, key)? } else { c.undefined() };
    if is_undefined(c, fields) {
        return Err(events::invalid_this(c, "Value of \"this\" must be of type MessageEvent"));
    }
    get(c, fields, MESSAGE_EVENT_FIELDS[index.min(MESSAGE_EVENT_FIELDS.len() - 1)])
}

/// Deliver a received message to `target`: Node-style listeners get the value, DOM ones a
/// MessageEvent carrying it and the transferred ports.
pub(super) fn dispatch_message(c: &mut Context<'_>, target: RootId, value: RootId, ports: RootId) -> Result<(), RootedError> {
    let kind = c.string_rooted(MESSAGE_EVENT_TYPE);
    let make = |c: &mut Context<'_>| {
        let class = member(c, member::MESSAGE_EVENT)?;
        let init = c.object_rooted()?;
        put(c, init, "data", value)?;
        put(c, init, "ports", ports)?;
        c.construct_rooted(class, class, &[kind, init])
    };
    events::hybrid_dispatch(c, target, value, kind, None, &make)
}

/// A closed port's 'close'.
pub(super) fn dispatch_close(c: &mut Context<'_>, port: RootId) -> Result<(), RootedError> {
    let kind = c.string_rooted("close");
    let undefined = c.undefined();
    let make = |c: &mut Context<'_>| {
        let data = events_api(c)?;
        let event = get(c, data, "eventCtor")?;
        c.construct_rooted(event, event, &[kind])
    };
    events::hybrid_dispatch(c, port, undefined, kind, None, &make)
}

/// `receiveMessageOnPort(port)`.
pub(crate) fn receive_message_on_port(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let port = argument(c, args, 0);
    let Some(id) = port_id(c, port)? else {
        return Err(coded(c, true, "ERR_INVALID_ARG_TYPE", "The \"port\" argument must be a MessagePort instance"));
    };
    super::receive(c, id)
}

/// `new BroadcastChannel(name)`.
pub(crate) fn broadcast_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    events::require_new(c, member::BROADCAST_CHANNEL)?;
    if args.is_empty() {
        return Err(events::missing_args(c, &["name"]));
    }
    let name = events::to_dom_string(c, args[0])?;
    events::initialize_node_target(c, receiver)?;
    let text = c.to_string(name)?;
    let id = super::subscribe(c, receiver, &text)?;
    let record = c.null_object_rooted()?;
    put(c, record, broadcast::NAME, name)?;
    let id = c.number(id as f64);
    put(c, record, broadcast::PORT, id)?;
    set_slot(c, receiver, record)?;
    let hook = member(c, member::PORT_HOOK)?;
    let data = events_api(c)?;
    let key = get(c, data, events::LISTENER_HOOK_SLOT)?;
    c.define_data_property_rooted(receiver, key, hook, true, false, true)?;
    Ok(receiver)
}

fn broadcast_record(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    let record = slot(c, receiver)?;
    let channel = member(c, member::BROADCAST_CHANNEL)?;
    match record {
        Some(record) if c.instance_of_rooted(receiver, channel)? => Ok(record),
        _ => Err(coded(c, true, "ERR_INVALID_THIS", "Value of \"this\" must be of type BroadcastChannel")),
    }
}

/// The port id of an open broadcast channel.
fn broadcast_port(c: &mut Context<'_>, receiver: RootId) -> Result<Option<u64>, RootedError> {
    let channel = member(c, member::BROADCAST_CHANNEL)?;
    if !c.is_object_rooted(receiver)? || !c.instance_of_rooted(receiver, channel)? {
        return Ok(None);
    }
    let Some(record) = slot(c, receiver)? else { return Ok(None) };
    let id = get(c, record, broadcast::PORT)?;
    Ok(c.rooted_value(id).and_then(|id| id.as_number()).map(|id| id as u64))
}

use crate::modules::keys::INSPECT_CUSTOM;

/// `[util.inspect.custom](depth, options)`: `BroadcastChannel { name, active }`.
pub(crate) fn broadcast_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let record = broadcast_record(c, receiver)?;
    let name = get(c, record, broadcast::NAME)?;
    let active = broadcast_port(c, receiver)?.is_some();
    let active = c.boolean(active);
    let data = c.object_rooted()?;
    put(c, data, "name", name)?;
    put(c, data, "active", active)?;
    crate::modules::webstreams_shared_vm::custom_inspect(c, receiver, member::BROADCAST_CHANNEL, data, args)
}

pub(crate) fn broadcast_name(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let record = broadcast_record(c, receiver)?;
    get(c, record, broadcast::NAME)
}

pub(crate) fn broadcast_close(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let record = broadcast_record(c, receiver)?;
    if let Some(id) = broadcast_port(c, receiver)? {
        super::unsubscribe(c, id);
        let undefined = c.undefined();
        put(c, record, broadcast::PORT, undefined)?;
    }
    Ok(c.undefined())
}

pub(crate) fn broadcast_post_message(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    broadcast_record(c, receiver)?;
    if args.is_empty() {
        return Err(events::missing_args(c, &["message"]));
    }
    let Some(id) = broadcast_port(c, receiver)? else {
        let message = c.string_rooted(BROADCAST_CLOSED);
        let name = c.string_rooted(INVALID_STATE_ERROR);
        let error = crate::modules::dom_exception_shared_vm::create(c, message, name)?;
        return Err(c.throw(error));
    };
    super::broadcast(c, id, args[0])?;
    Ok(c.undefined())
}

fn broadcast_reference(c: &mut Context<'_>, receiver: RootId, referenced: bool) -> Result<RootId, RootedError> {
    broadcast_record(c, receiver)?;
    if let Some(id) = broadcast_port(c, receiver)? {
        super::with_port(c, id, |port| port.referenced = referenced);
    }
    Ok(receiver)
}

pub(crate) fn broadcast_ref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    broadcast_reference(c, receiver, true)
}

pub(crate) fn broadcast_unref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    broadcast_reference(c, receiver, false)
}

/// The Worker accessors, in Node's order; data is the index.
const WORKER_GETTERS: [&str; 6] = ["threadId", "threadName", "stdin", "stdout", "stderr", "resourceLimits"];

fn worker_record(c: &mut Context<'_>, receiver: RootId) -> Result<RootId, RootedError> {
    let class = member(c, member::WORKER)?;
    let record = slot(c, receiver)?;
    match record {
        Some(record) if c.instance_of_rooted(receiver, class)? => Ok(record),
        _ => {
            let error = c.type_error_rooted("Illegal invocation")?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn worker_getter(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let index = c.host_function_data()?;
    let index = c.to_number(index)? as usize;
    let record = worker_record(c, receiver)?;
    match WORKER_GETTERS.get(index).copied() {
        Some("threadId") => get(c, record, worker::THREAD_ID),
        Some("threadName") => Ok(c.string_rooted(WORKER_THREAD_NAME)),
        Some("stdout") => get(c, record, worker::STDOUT),
        Some("stderr") => get(c, record, worker::STDERR),
        Some("resourceLimits") => c.object_rooted(),
        _ => Ok(c.null()),
    }
}

/// A Worker method Quench answers as Node does for a worker no longer running.
pub(crate) fn worker_not_running(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let error = coded(c, false, "ERR_WORKER_NOT_RUNNING", "Worker instance not running");
    let promise = c.pending_promise_rooted()?;
    if let Some(reason) = error.exception {
        c.reject_promise_rooted(promise, reason)?;
        return Ok(promise);
    }
    Err(error)
}

/// The worker entry `new Worker(filename, options)` names: an eval source, or an absolute path.
fn worker_entry(c: &mut Context<'_>, filename: RootId, is_eval: bool) -> Result<String, RootedError> {
    let url = crate::host::shared_vm::commonjs::url_module(c)?;
    let url_class = get(c, url, "URL")?;
    if c.is_object_rooted(filename)? && c.instance_of_rooted(filename, url_class)? {
        let protocol = get(c, filename, "protocol")?;
        if c.string_text(protocol)?.as_deref() != Some("file:") {
            return Err(coded(c, true, "ERR_WORKER_UNSUPPORTED_OPERATION", "Unsupported Worker URL protocol"));
        }
        let to_path = get(c, url, "fileURLToPath")?;
        let path = c.call_rooted(to_path, url, &[filename])?;
        return c.to_string(path);
    }
    let Some(text) = c.string_text(filename)? else {
        return Err(argument_type_error_with_expected(c, filename, "filename", "of type string or an instance of URL")?);
    };
    if is_eval {
        return Ok(text);
    }
    let path = crate::host::shared_vm::commonjs::path_module(c)?;
    let absolute = std::path::Path::new(&text).is_absolute();
    if !absolute && !RELATIVE_PREFIXES.iter().any(|prefix| text.starts_with(prefix)) {
        let message = format!(
            "The worker script or module filename must be an absolute path or a relative path starting with './' or '../'. Received \"{text}\""
        );
        return Err(coded(c, true, "ERR_WORKER_PATH", &message));
    }
    let text_root = c.string_rooted(&text);
    let resolved = call_method(c, path, "resolve", &[text_root])?;
    c.to_string(resolved)
}

fn string_list(c: &mut Context<'_>, options: RootId, name: &str) -> Result<Option<Vec<String>>, RootedError> {
    let list = get(c, options, name)?;
    if is_undefined(c, list) {
        return Ok(None);
    }
    if !c.is_array_rooted(list)? {
        return Err(argument_type_error(c, list, &format!("options.{name}"), "Array")?);
    }
    let mut strings = Vec::new();
    for value in c.array_like_values_rooted(list)? {
        strings.push(c.to_string(value)?);
    }
    Ok(Some(strings))
}

fn share_env(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = c.string_rooted(SHARE_ENV_KEY);
    call_method(c, symbol, "for", &[key])
}

/// The worker's environment: `options.env`, or its parent's for `SHARE_ENV` and nullish.
fn worker_env(c: &mut Context<'_>, options: RootId) -> Result<Vec<(String, String)>, RootedError> {
    let env = get(c, options, "env")?;
    let shared = share_env(c)?;
    let source = if is_nullish(c, env) || c.strict_equal_rooted(env, shared)? {
        let process = c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
        get(c, process, "env")?
    } else if c.is_object_rooted(env)? {
        env
    } else {
        let received = crate::modules::util_shared_vm::argument_type_description(c, env)?;
        let message = format!(
            "The \"options.env\" property must be of type object or one of undefined, null, or worker_threads.SHARE_ENV. Received {received}"
        );
        return Err(coded(c, true, "ERR_INVALID_ARG_TYPE", &message));
    };
    let mut pairs = Vec::new();
    for key in c.enumerable_own_keys_rooted(source, false, true)? {
        let value = c.get_property_rooted(source, key)?;
        pairs.push((c.to_string(key)?, c.to_string(value)?));
    }
    Ok(pairs)
}

/// A Readable the worker's output is pushed to.
fn output_stream(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let stream = crate::host::shared_vm::commonjs::stream_module(c)?;
    let readable = get(c, stream, "Readable")?;
    let options = c.object_rooted()?;
    let read = c.host_function_named(crate::host::shared_vm::operation!("streamNop"), "read")?;
    put(c, options, "read", read)?;
    c.construct_rooted(readable, readable, &[options])
}

/// `new Worker(filename, options)`.
pub(crate) fn worker_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    events::require_new(c, member::WORKER)?;
    let emitter = crate::host::shared_vm::commonjs::events_module(c)?;
    c.call_rooted(emitter, receiver, &[])?;
    // Node reads the options' members without validating the options themselves.
    let options = argument(c, args, 1);
    let options = if c.is_object_rooted(options)? { options } else { c.object_rooted()? };
    let is_eval = get(c, options, "eval")?;
    let is_eval = c.truthy_rooted(is_eval)?;
    let filename = argument(c, args, 0);
    let entry = worker_entry(c, filename, is_eval)?;
    let argv = string_list(c, options, "argv")?.unwrap_or_default();
    let exec_argv = string_list(c, options, "execArgv")?;
    let env = worker_env(c, options)?;
    let list = get(c, options, "transferList")?;
    let items = transfer_list(c, list, false)?;
    let transfers = split_transfers(c, items)?;
    let data = get(c, options, "workerData")?;
    let environment = member(c, member::ENVIRONMENT)?;
    let launch = super::WorkerStart { entry, is_eval, argv, exec_argv, env, data, transfers, environment };
    let (thread_id, port) = super::start_worker(c, receiver, launch)?;
    let record = c.null_object_rooted()?;
    let thread_id = c.number(thread_id as f64);
    put(c, record, worker::THREAD_ID, thread_id)?;
    put(c, record, worker::PORT, port)?;
    let yes = c.boolean(true);
    put(c, record, worker::REFERENCED, yes)?;
    let process = c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
    for (name, option) in [(worker::STDOUT, "stdout"), (worker::STDERR, "stderr")] {
        let stream = output_stream(c)?;
        put(c, record, name, stream)?;
        let captured = get(c, options, option)?;
        if !c.truthy_rooted(captured)? {
            let destination = get(c, process, option)?;
            let write = get(c, destination, "write")?;
            let bound = call_method(c, write, "bind", &[destination])?;
            crate::modules::stream_shared_vm::on(c, stream, "data", bound)?;
        }
    }
    set_slot(c, receiver, record)?;
    let resource = c.object_rooted()?;
    let has_ref = c.host_function_named_with_data(crate::host::shared_vm::operation!("workerResourceHasRef"), "hasRef", record)?;
    put(c, resource, "hasRef", has_ref)?;
    emit_init(c, resource, WORKER_RESOURCE)?;
    for event in ["message", "messageerror"] {
        let event_name = c.string_rooted(event);
        let data = c.array_rooted(&[receiver, event_name])?;
        let forward = c.host_function_named_with_data(crate::host::shared_vm::operation!("workerForward"), "", data)?;
        crate::modules::stream_shared_vm::on(c, port, event, forward)?;
    }
    port_unref(c, port, &[])?;
    Ok(receiver)
}

pub(crate) fn worker_resource_has_ref(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let record = c.host_function_data()?;
    get(c, record, worker::REFERENCED)
}

/// A parent-side port listener re-emitting on the worker: `worker.emit(event, value)`.
pub(crate) fn worker_forward(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let worker = get(c, data, "0")?;
    let event = get(c, data, "1")?;
    let value = argument(c, args, 0);
    call_method(c, worker, "emit", &[event, value])
}

pub(crate) fn worker_post_message(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let record = worker_record(c, receiver)?;
    let port = get(c, record, worker::PORT)?;
    port_post_message(c, port, args)
}

fn worker_reference(c: &mut Context<'_>, receiver: RootId, referenced: bool) -> Result<RootId, RootedError> {
    let record = worker_record(c, receiver)?;
    let flag = c.boolean(referenced);
    put(c, record, worker::REFERENCED, flag)?;
    super::set_worker_referenced(c, receiver, referenced)?;
    Ok(c.undefined())
}

pub(crate) fn worker_ref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    worker_reference(c, receiver, true)
}

pub(crate) fn worker_unref(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    worker_reference(c, receiver, false)
}

/// `worker.terminate(callback)`: a promise of the exit code, also passed to `callback`.
pub(crate) fn worker_terminate(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let record = worker_record(c, receiver)?;
    let exit = get(c, record, worker::EXIT)?;
    let exit = if is_undefined(c, exit) {
        let promise = c.pending_promise_rooted()?;
        put(c, record, worker::EXIT, promise)?;
        if super::worker_exited(c, receiver)? {
            let undefined = c.undefined();
            c.resolve_promise_rooted(promise, undefined)?;
        } else {
            let resolve = c.host_function_named_with_data(crate::host::shared_vm::operation!("workerResolveExit"), "", promise)?;
            crate::modules::stream_shared_vm::once(c, receiver, "exit", resolve)?;
        }
        promise
    } else {
        exit
    };
    super::terminate_worker(c, receiver)?;
    let callback = argument(c, args, 0);
    if c.is_callable_rooted(callback)? {
        let report = c.host_function_named_with_data(crate::host::shared_vm::operation!("workerTerminateCallback"), "", callback)?;
        call_method(c, exit, "then", &[report])?;
    }
    Ok(exit)
}

pub(crate) fn worker_resolve_exit(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let promise = c.host_function_data()?;
    let code = argument(c, args, 0);
    c.resolve_promise_rooted(promise, code)?;
    Ok(c.undefined())
}

pub(crate) fn worker_terminate_callback(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = c.host_function_data()?;
    let null = c.null();
    let code = argument(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[null, code])
}

/// A worker lifecycle event from the event loop.
pub(super) enum WorkerEvent {
    Online,
    Output { stderr: bool, chunk: RootId },
    Error(RootId),
    Exit(i32),
}

pub(super) fn worker_event(c: &mut Context<'_>, worker: RootId, event: WorkerEvent) -> Result<(), RootedError> {
    let record = worker_record(c, worker)?;
    match event {
        WorkerEvent::Online => {
            let name = c.string_rooted("online");
            call_method(c, worker, "emit", &[name])?;
        }
        WorkerEvent::Error(error) => {
            let name = c.string_rooted("error");
            call_method(c, worker, "emit", &[name, error])?;
        }
        WorkerEvent::Output { stderr, chunk } => {
            let stream = get(c, record, if stderr { worker::STDERR } else { worker::STDOUT })?;
            call_method(c, stream, "push", &[chunk])?;
        }
        WorkerEvent::Exit(code) => {
            let port = get(c, record, worker::PORT)?;
            port_close(c, port, &[])?;
            let end = c.null();
            for name in [worker::STDOUT, worker::STDERR] {
                let stream = get(c, record, name)?;
                call_method(c, stream, "push", &[end])?;
            }
            let no = c.boolean(false);
            put(c, record, worker::REFERENCED, no)?;
            let exited = c.number(EXITED_THREAD_ID);
            put(c, record, worker::THREAD_ID, exited)?;
            let name = c.string_rooted("exit");
            let code = c.number(f64::from(code));
            call_method(c, worker, "emit", &[name, code])?;
        }
    }
    Ok(())
}

/// `setEnvironmentData(key, value)`: undefined removes the key.
pub(crate) fn set_environment_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let map = member(c, member::ENVIRONMENT)?;
    let key = argument(c, args, 0);
    let value = argument(c, args, 1);
    if is_undefined(c, value) {
        call_method(c, map, "delete", &[key])?;
    } else {
        call_method(c, map, "set", &[key, value])?;
    }
    Ok(c.undefined())
}

pub(crate) fn get_environment_data(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let map = member(c, member::ENVIRONMENT)?;
    let key = argument(c, args, 0);
    call_method(c, map, "get", &[key])
}

/// A worker receives its parent's environment data as it was at the worker's creation.
pub(super) fn adopt_environment(c: &mut Context<'_>, inherited: RootId) -> Result<(), RootedError> {
    let api = super::api(c)?;
    put(c, api, member::ENVIRONMENT, inherited)
}

pub(crate) fn move_message_port_to_context(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Err(coded(c, false, "ERR_METHOD_NOT_IMPLEMENTED", "The moveMessagePortToContext() method is not implemented"))
}

/// `postMessageToThread`: Quench delivers no cross-thread messages outside ports.
pub(crate) fn post_message_to_thread(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let error = coded(c, false, "ERR_WORKER_MESSAGING_FAILED", "Cannot find the destination thread or listener");
    let promise = c.pending_promise_rooted()?;
    if let Some(reason) = error.exception {
        c.reject_promise_rooted(promise, reason)?;
        return Ok(promise);
    }
    Err(error)
}

/// What a VM's `worker_threads` module reports about its thread.
pub(super) struct ThreadFacts {
    pub(super) is_main: bool,
    pub(super) thread_id: u64,
    pub(super) parent_port: RootId,
    pub(super) data: RootId,
}

/// The `worker_threads` exports, in Node's order.
pub(super) fn module(c: &mut Context<'_>, facts: ThreadFacts) -> Result<RootId, RootedError> {
    let api = super::api(c)?;
    let module = c.object_rooted()?;
    let no = c.boolean(false);
    put(c, module, "isInternalThread", no)?;
    let is_main = c.boolean(facts.is_main);
    put(c, module, "isMainThread", is_main)?;
    for name in [member::MESSAGE_PORT, member::MESSAGE_CHANNEL] {
        let class = get(c, api, name)?;
        put(c, module, name, class)?;
    }
    let function = |c: &mut Context<'_>, module: RootId, name: &str, op: &str| -> Result<(), RootedError> {
        let function = c.host_function_named(operation(op), name)?;
        put(c, module, name, function)
    };
    for name in ["markAsUncloneable", "markAsUntransferable", "isMarkedAsUntransferable"] {
        function(c, module, name, name)?;
    }
    function(c, module, "moveMessagePortToContext", "workerMoveMessagePortToContext")?;
    function(c, module, "receiveMessageOnPort", "messagingReceiveMessageOnPort")?;
    let limits = c.object_rooted()?;
    put(c, module, "resourceLimits", limits)?;
    function(c, module, "postMessageToThread", "workerPostMessageToThread")?;
    let thread_id = c.number(facts.thread_id as f64);
    put(c, module, "threadId", thread_id)?;
    let name = c.string_rooted(if facts.is_main { MAIN_THREAD_NAME } else { WORKER_THREAD_NAME });
    put(c, module, "threadName", name)?;
    let shared = share_env(c)?;
    put(c, module, "SHARE_ENV", shared)?;
    let worker = get(c, api, member::WORKER)?;
    put(c, module, "Worker", worker)?;
    put(c, module, "parentPort", facts.parent_port)?;
    put(c, module, "workerData", facts.data)?;
    let broadcast = get(c, api, member::BROADCAST_CHANNEL)?;
    put(c, module, "BroadcastChannel", broadcast)?;
    function(c, module, "setEnvironmentData", "workerEnvironmentSet")?;
    function(c, module, "getEnvironmentData", "workerEnvironmentGet")?;
    Ok(module)
}

/// The classes a global group installs (`MessagePort`, `MessageChannel`, `BroadcastChannel`).
pub(super) fn global_class(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    member(c, name)
}
