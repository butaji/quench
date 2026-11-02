//! `diagnostics_channel` (`lib/diagnostics_channel.js`): named channels, their bound stores
//! and tracing channels.
//!
//! The host registry (`SharedDiagnosticsState`) owns every channel's subscribers: the
//! module's `publish` and the host's publishers (HTTP, net, child_process, module loading)
//! deliver through the same lists. A channel object carries its registry key in a private
//! slot. Its prototype is its activity state, as in Node: `subscribe` and `bindStore` switch
//! it to `ActiveChannel`, the last removal back to `Channel`; `_subscribers` projects the
//! registry list and `_stores` holds its bound stores.
use super::event_target_shared_vm::{argument, get, is_nullish, is_undefined, put, queue_uncaught, require_new};
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError, ValueType};
use std::collections::HashMap;

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

#[derive(Default)]
pub(crate) struct SharedDiagnosticsState {
    /// The subscribers of each channel named by a string, in subscription order.
    pub(crate) channels: HashMap<String, Vec<RootId>>,
    /// The subscribers of each channel named by another value, by the ordinal it carries.
    identified: HashMap<u64, Vec<RootId>>,
    next_ordinal: u64,
    api: Option<RootId>,
    pub(crate) module: Option<RootId>,
}

/// A channel's registry key: its string name, or the ordinal a channel named otherwise
/// (a symbol) was given, since only strings key the registry by value.
#[derive(Clone, Copy)]
enum ChannelKey<'a> {
    Name(&'a str),
    Ordinal(u64),
}

impl SharedDiagnosticsState {
    fn subscribers(&self, key: ChannelKey<'_>) -> &[RootId] {
        let list = match key {
            ChannelKey::Name(name) => self.channels.get(name),
            ChannelKey::Ordinal(ordinal) => self.identified.get(&ordinal),
        };
        list.map_or(&[], Vec::as_slice)
    }

    fn subscribers_mut(&mut self, key: ChannelKey<'_>) -> &mut Vec<RootId> {
        match key {
            ChannelKey::Name(name) => self.channels.entry(name.to_owned()).or_default(),
            ChannelKey::Ordinal(ordinal) => self.identified.entry(ordinal).or_default(),
        }
    }

    fn remove_if_empty(&mut self, key: ChannelKey<'_>) {
        match key {
            ChannelKey::Name(name) => {
                if self.channels.get(name).is_some_and(Vec::is_empty) {
                    self.channels.remove(name);
                }
            }
            ChannelKey::Ordinal(ordinal) => {
                if self.identified.get(&ordinal).is_some_and(Vec::is_empty) {
                    self.identified.remove(&ordinal);
                }
            }
        }
    }
}

/// A class member: (name, operation, length).
type Method = (&'static str, &'static str, f64);

/// `Channel.prototype` and `ActiveChannel.prototype` share one operation per member (each
/// state is the other with nothing subscribed or bound), with Node's per-class lengths.
const CHANNEL_METHODS: [Method; 6] = [
    ("subscribe", "diagnosticsChannelSubscribe", 1.0),
    ("unsubscribe", "diagnosticsChannelUnsubscribe", 0.0),
    ("bindStore", "diagnosticsChannelBindStore", 2.0),
    ("unbindStore", "diagnosticsChannelUnbindStore", 0.0),
    ("publish", "diagnosticsChannelPublish", 0.0),
    ("runStores", "diagnosticsChannelRunStores", 3.0),
];
const ACTIVE_CHANNEL_METHODS: [Method; 6] = [
    ("subscribe", "diagnosticsChannelSubscribe", 1.0),
    ("unsubscribe", "diagnosticsChannelUnsubscribe", 1.0),
    ("bindStore", "diagnosticsChannelBindStore", 2.0),
    ("unbindStore", "diagnosticsChannelUnbindStore", 1.0),
    ("publish", "diagnosticsChannelPublish", 1.0),
    ("runStores", "diagnosticsChannelRunStores", 3.0),
];
/// The members before `hasSubscribers` in each channel prototype's key order.
const MEMBERS_BEFORE_HAS_SUBSCRIBERS: usize = 4;
const TRACING_METHODS: [Method; 5] = [
    ("subscribe", "diagnosticsTracingSubscribe", 1.0),
    ("unsubscribe", "diagnosticsTracingUnsubscribe", 1.0),
    ("traceSync", "diagnosticsTraceSync", 1.0),
    ("tracePromise", "diagnosticsTracePromise", 1.0),
    ("traceCallback", "diagnosticsTraceCallback", 1.0),
];
/// The module's functions after `channel`, in Node's export order.
const FUNCTIONS: [Method; 5] = [
    ("channel", "diagnosticsChannel", 1.0),
    ("hasSubscribers", "diagnosticsHasSubscribers", 1.0),
    ("subscribe", "diagnosticsSubscribe", 2.0),
    ("tracingChannel", "diagnosticsTracingChannel", 1.0),
    ("unsubscribe", "diagnosticsUnsubscribe", 2.0),
];

/// A channel's own fields, in Node's definition order.
const SUBSCRIBERS: &str = "_subscribers";
const STORES: &str = "_stores";
const NAME: &str = "name";

/// The members of the group's shared data (`SharedDiagnosticsState::api`).
mod api {
    /// The private key of a channel's registry key.
    pub(super) const SLOT: &str = "slot";
    /// The channels by name (a `Map`), as `channel(name)` finds them.
    pub(super) const CHANNELS: &str = "channels";
    pub(super) const MAP: &str = "Map";
    pub(super) const MAP_GET: &str = "mapGet";
    pub(super) const MAP_SET: &str = "mapSet";
    pub(super) const MAP_DELETE: &str = "mapDelete";
    pub(super) const STORE_MAP_PROTOTYPE: &str = "storeMapPrototype";
    pub(super) const CHANNEL_PROTOTYPE: &str = "channelPrototype";
    pub(super) const ACTIVE_PROTOTYPE: &str = "activePrototype";
    pub(super) const CHANNEL_CLASS: &str = "channelClass";
    pub(super) const TRACING_CLASS: &str = "tracingClass";
    pub(super) const PROMISE: &str = "Promise";
    pub(super) const PROMISE_RESOLVE: &str = "promiseResolve";
    pub(super) const PROMISE_REJECT: &str = "promiseReject";
    pub(super) const PROMISE_THEN: &str = "promiseThen";
}

/// A traced call: the call's own fields, then its tracing channel's event channels, which
/// are named after the events they carry (`TRACE_EVENTS`).
const TRACE_FRAME: [&str; 9] =
    ["function", "receiver", "arguments", "context", "start", "end", "asyncStart", "asyncEnd", "error"];
const TRACE_CALL_FIELDS: usize = 4;
const TRACE_EVENTS: &[&str] = TRACE_FRAME.split_at(TRACE_CALL_FIELDS).1;
/// A `runStores` call: the channel, its message and the call it wraps, and the stores bound
/// when it began (`[store, transform, ...]`).
const STORE_RUN: [&str; 6] = ["channel", "data", "function", "receiver", "arguments", "stores"];
/// A store run's continuation: the run and the stores still to enter.
const STORE_STEP: [&str; 2] = ["run", "remaining"];
/// A traced callback's wrapper and its continuation's call.
const WRAPPED_CALLBACK: [&str; 2] = ["frame", "callback"];
const CALLBACK_CALL: [&str; 4] = ["frame", "callback", "receiver", "arguments"];
/// `traceCallback`'s default callback position: the last argument.
const LAST_ARGUMENT: f64 = -1.0;
/// A bound store's entry in a `Map` preview: the store, then its transform.
const STORE_ENTRY_WIDTH: usize = 2;

fn operation(name: &str) -> quench_runtime::HostFunctionId {
    crate::host::shared_vm::operation(name)
}

fn shared_api(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let api = c.host_mut().shared_state().borrow().diagnostics.api;
    match api {
        Some(api) => Ok(api),
        None => {
            module(c)?;
            api_data(c)
        }
    }
}

fn api_data(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .diagnostics
        .api
        .ok_or_else(|| RootedError::host("diagnostics_channel data missing"))
}

fn api_member(c: &mut Context<'_>, name: &str) -> Outcome {
    let api = shared_api(c)?;
    get(c, api, name)
}

fn define_methods(c: &mut Context<'_>, holder: RootId, methods: &[Method]) -> Result<(), RootedError> {
    for &(name, op, length) in methods {
        let function = c.host_function_named(operation(op), name)?;
        let length = c.number(length);
        c.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        c.define_named_data_property_rooted(holder, name, function, true, false, true)?;
    }
    Ok(())
}

fn define_getter(c: &mut Context<'_>, holder: RootId, name: &str, op: &str, data: RootId) -> Result<(), RootedError> {
    let getter = c.host_function_named_with_data(operation(op), &format!("get {name}"), data)?;
    let key = c.string_rooted(name);
    let undefined = c.undefined();
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(undefined),
        enumerable: Some(false),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(holder, key, fields)?;
    Ok(())
}

/// A class as a declaration makes it: its prototype's methods non-enumerable, `prototype`
/// read-only.
fn class(c: &mut Context<'_>, name: &str, constructor: &str, length: f64) -> Outcome {
    let class = c.host_constructor_named(operation(constructor), name)?;
    let length = c.number(length);
    c.define_named_data_property_rooted(class, "length", length, false, false, true)?;
    let prototype = get(c, class, "prototype")?;
    c.define_named_data_property_rooted(class, "prototype", prototype, false, false, false)?;
    Ok(class)
}

/// A channel prototype: Node's members, `hasSubscribers` answering `active`.
fn channel_prototype(c: &mut Context<'_>, class: RootId, methods: &[Method], active: bool) -> Outcome {
    let prototype = get(c, class, "prototype")?;
    let (before, after) = methods.split_at(MEMBERS_BEFORE_HAS_SUBSCRIBERS);
    define_methods(c, prototype, before)?;
    let active = c.boolean(active);
    define_getter(c, prototype, "hasSubscribers", "diagnosticsChannelHasSubscribers", active)?;
    define_methods(c, prototype, after)?;
    Ok(prototype)
}

fn intrinsic(c: &mut Context<'_>, holder: RootId, name: &str) -> Outcome {
    match c.startup_data_property_rooted(holder, name) {
        Some(value) => Ok(value),
        None => get(c, holder, name),
    }
}

/// The module's exports, in Node's order; the classes and intrinsics it uses are kept in
/// the group's data.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    if let Some(module) = c.host_mut().shared_state().borrow().diagnostics.module {
        return Ok(module);
    }
    let api = c.null_object_rooted()?;
    let slot = c.private_symbol_rooted(Some("diagnostics channel key"));
    put(c, api, api::SLOT, slot)?;
    let global = c.global_root()?;
    let map = intrinsic(c, global, "Map")?;
    let map_prototype = intrinsic(c, map, "prototype")?;
    put(c, api, api::MAP, map)?;
    for (member, method) in [(api::MAP_GET, "get"), (api::MAP_SET, "set"), (api::MAP_DELETE, "delete")] {
        let function = intrinsic(c, map_prototype, method)?;
        put(c, api, member, function)?;
    }
    let channels = c.construct_rooted(map, map, &[])?;
    put(c, api, api::CHANNELS, channels)?;
    // Node keeps bound stores in a `SafeMap`, a `Map` its inspection names so.
    let store_map = class(c, "SafeMap", "diagnosticsInertConstructor", 0.0)?;
    let store_map_prototype = get(c, store_map, "prototype")?;
    c.set_prototype_rooted(store_map_prototype, map_prototype)?;
    put(c, api, api::STORE_MAP_PROTOTYPE, store_map_prototype)?;
    let promise = intrinsic(c, global, "Promise")?;
    put(c, api, api::PROMISE, promise)?;
    let promise_prototype = intrinsic(c, promise, "prototype")?;
    for (member, holder, method) in [
        (api::PROMISE_RESOLVE, promise, "resolve"),
        (api::PROMISE_REJECT, promise, "reject"),
        (api::PROMISE_THEN, promise_prototype, "then"),
    ] {
        let function = intrinsic(c, holder, method)?;
        put(c, api, member, function)?;
    }

    let channel_class = class(c, "Channel", "diagnosticsChannelConstructor", 1.0)?;
    let prototype = channel_prototype(c, channel_class, &CHANNEL_METHODS, false)?;
    put(c, api, api::CHANNEL_PROTOTYPE, prototype)?;
    put(c, api, api::CHANNEL_CLASS, channel_class)?;
    let symbol = intrinsic(c, global, "Symbol")?;
    let has_instance = intrinsic(c, symbol, "hasInstance")?;
    let is_channel = c.host_function_named(operation("diagnosticsChannelHasInstance"), "[Symbol.hasInstance]")?;
    c.define_data_property_rooted(channel_class, has_instance, is_channel, true, false, true)?;
    let active_class = class(c, "ActiveChannel", "diagnosticsInertConstructor", 0.0)?;
    let active = channel_prototype(c, active_class, &ACTIVE_CHANNEL_METHODS, true)?;
    put(c, api, api::ACTIVE_PROTOTYPE, active)?;

    let tracing_class = class(c, "TracingChannel", "diagnosticsTracingConstructor", 1.0)?;
    let tracing_prototype = get(c, tracing_class, "prototype")?;
    let undefined = c.undefined();
    define_getter(c, tracing_prototype, "hasSubscribers", "diagnosticsTracingHasSubscribers", undefined)?;
    define_methods(c, tracing_prototype, &TRACING_METHODS)?;
    put(c, api, api::TRACING_CLASS, tracing_class)?;

    let retained = c.retain(api)?;
    c.host_mut().shared_state().borrow_mut().diagnostics.api = Some(retained);
    let module = c.object_rooted()?;
    define_functions(c, module)?;
    put(c, module, "Channel", channel_class)?;
    let retained = c.retain(module)?;
    c.host_mut().shared_state().borrow_mut().diagnostics.module = Some(retained);
    Ok(module)
}

fn define_functions(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    for &(name, op, length) in &FUNCTIONS {
        let function = c.host_function_named(operation(op), name)?;
        let length = c.number(length);
        c.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        put(c, module, name, function)?;
    }
    Ok(())
}

pub(crate) fn inert_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    require_new(c, "ActiveChannel")?;
    Ok(c.undefined())
}

fn call_method(c: &mut Context<'_>, receiver: RootId, name: &str, args: &[RootId]) -> Outcome {
    let method = get(c, receiver, name)?;
    c.call_rooted(method, receiver, args)
}

fn channels_get(c: &mut Context<'_>, name: RootId) -> Outcome {
    let channels = api_member(c, api::CHANNELS)?;
    let map_get = api_member(c, api::MAP_GET)?;
    c.call_rooted(map_get, channels, &[name])
}

/// `new Channel(name)`: an inactive channel, which `channel(name)` finds from now on.
pub(crate) fn channel_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    require_new(c, "Channel")?;
    let name = argument(c, args, 0);
    let undefined = c.undefined();
    put(c, receiver, SUBSCRIBERS, undefined)?;
    put(c, receiver, STORES, undefined)?;
    put(c, receiver, NAME, name)?;
    let key = if c.value_type_rooted(name) == Some(ValueType::String) {
        name
    } else {
        let shared = c.host_mut().shared_state();
        let ordinal = {
            let mut state = shared.borrow_mut();
            let ordinal = state.diagnostics.next_ordinal;
            state.diagnostics.next_ordinal += 1;
            ordinal
        };
        c.number(ordinal as f64)
    };
    let slot = api_member(c, api::SLOT)?;
    c.define_data_property_rooted(receiver, slot, key, false, false, false)?;
    let channels = api_member(c, api::CHANNELS)?;
    let map_set = api_member(c, api::MAP_SET)?;
    c.call_rooted(map_set, channels, &[name, receiver])?;
    Ok(undefined)
}

/// The registry key a channel carries, text holding a string name; none for another value.
fn channel_key(c: &mut Context<'_>, channel: RootId) -> Result<Option<(Option<String>, u64)>, RootedError> {
    if !c.is_object_rooted(channel)? {
        return Ok(None);
    }
    let slot = api_member(c, api::SLOT)?;
    let key = c.get_property_rooted(channel, slot)?;
    if let Some(name) = c.string_text(key)? {
        return Ok(Some((Some(name), 0)));
    }
    Ok(c.rooted_value(key).and_then(|value| value.as_number()).map(|ordinal| (None, ordinal as u64)))
}

fn borrow_key(key: &(Option<String>, u64)) -> ChannelKey<'_> {
    match key {
        (Some(name), _) => ChannelKey::Name(name),
        (None, ordinal) => ChannelKey::Ordinal(*ordinal),
    }
}

/// `Channel[Symbol.hasInstance]`: a channel in either state.
pub(crate) fn channel_has_instance(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = argument(c, args, 0);
    let is_channel = is_channel(c, value)?;
    Ok(c.boolean(is_channel))
}

/// `ObjectGetPrototypeOf(value)` is `Channel.prototype` or `ActiveChannel.prototype`.
fn is_channel(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if is_nullish(c, value) {
        let error = c.type_error_rooted("Cannot convert undefined or null to object")?;
        return Err(c.throw(error));
    }
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let prototype = c.prototype_rooted(value)?;
    for member in [api::CHANNEL_PROTOTYPE, api::ACTIVE_PROTOTYPE] {
        let candidate = api_member(c, member)?;
        if c.strict_equal_rooted(prototype, candidate)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_active(c: &mut Context<'_>, channel: RootId) -> Result<bool, RootedError> {
    let prototype = c.prototype_rooted(channel)?;
    let active = api_member(c, api::ACTIVE_PROTOTYPE)?;
    c.strict_equal_rooted(prototype, active)
}

/// `markActive`: an inactive channel becomes active with no subscriber or store.
fn activate(c: &mut Context<'_>, channel: RootId) -> Result<(), RootedError> {
    if is_active(c, channel)? {
        return Ok(());
    }
    let active = api_member(c, api::ACTIVE_PROTOTYPE)?;
    c.set_prototype_rooted(channel, active)?;
    let subscribers = c.array_rooted(&[])?;
    put(c, channel, SUBSCRIBERS, subscribers)?;
    let map = api_member(c, api::MAP)?;
    let stores = c.construct_rooted(map, map, &[])?;
    let store_map = api_member(c, api::STORE_MAP_PROTOTYPE)?;
    c.set_prototype_rooted(stores, store_map)?;
    put(c, channel, STORES, stores)
}

/// The bound stores of `channel`, `[store, transform, ...]` in binding order.
fn store_entries(c: &mut Context<'_>, channel: RootId) -> Result<Vec<RootId>, RootedError> {
    let stores = get(c, channel, STORES)?;
    if !c.is_object_rooted(stores)? {
        return Ok(Vec::new());
    }
    Ok(c.collection_preview_rooted(stores)?.map(|preview| preview.entries).unwrap_or_default())
}

/// `maybeMarkInactive`: a channel with no subscriber and no store becomes inactive.
fn deactivate_if_idle(c: &mut Context<'_>, channel: RootId, key: ChannelKey<'_>) -> Result<(), RootedError> {
    let subscribed = !c.host_mut().shared_state().borrow().diagnostics.subscribers(key).is_empty();
    if subscribed || !store_entries(c, channel)?.is_empty() {
        return Ok(());
    }
    let inactive = api_member(c, api::CHANNEL_PROTOTYPE)?;
    c.set_prototype_rooted(channel, inactive)?;
    let undefined = c.undefined();
    put(c, channel, SUBSCRIBERS, undefined)?;
    put(c, channel, STORES, undefined)
}

/// `_subscribers` after a change: a new array of the registry list, as Node copies on write.
fn project_subscribers(c: &mut Context<'_>, channel: RootId, key: ChannelKey<'_>) -> Result<(), RootedError> {
    let subscribers = c.host_mut().shared_state().borrow().diagnostics.subscribers(key).to_vec();
    let subscribers = c.array_rooted(&subscribers)?;
    put(c, channel, SUBSCRIBERS, subscribers)
}

/// `subscribe(subscription)`: `subscription` joins the channel's subscribers, again if it is
/// one already.
pub(crate) fn channel_subscribe(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let subscription = argument(c, args, 0);
    let Some(key) = channel_key(c, receiver)? else {
        return Ok(c.undefined());
    };
    activate(c, receiver)?;
    if !c.is_callable_rooted(subscription)? {
        return Err(super::util_shared_vm::argument_type_error(c, subscription, "subscription", "function")?);
    }
    let retained = c.retain(subscription)?;
    let key = borrow_key(&key);
    c.host_mut().shared_state().borrow_mut().diagnostics.subscribers_mut(key).push(retained);
    project_subscribers(c, receiver, key)?;
    Ok(c.undefined())
}

/// `unsubscribe(subscription)`: whether the first subscription of `subscription` was removed.
pub(crate) fn channel_unsubscribe(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let subscription = argument(c, args, 0);
    let Some(key) = channel_key(c, receiver)? else {
        return Ok(c.boolean(false));
    };
    let key = borrow_key(&key);
    let subscribers = c.host_mut().shared_state().borrow().diagnostics.subscribers(key).to_vec();
    let mut found = None;
    for (index, subscriber) in subscribers.into_iter().enumerate() {
        if c.strict_equal_rooted(subscriber, subscription)? {
            found = Some(index);
            break;
        }
    }
    let Some(index) = found else {
        return Ok(c.boolean(false));
    };
    let removed = {
        let shared = c.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        let removed = state.diagnostics.subscribers_mut(key).remove(index);
        state.diagnostics.remove_if_empty(key);
        removed
    };
    c.release_root(removed);
    project_subscribers(c, receiver, key)?;
    deactivate_if_idle(c, receiver, key)?;
    Ok(c.boolean(true))
}

/// `bindStore(store, transform)`: `runStores` enters `store` with `transform(data)`; binding a
/// bound store replaces its transform.
pub(crate) fn channel_bind_store(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    if channel_key(c, receiver)?.is_none() {
        return Ok(c.undefined());
    }
    activate(c, receiver)?;
    let store = argument(c, args, 0);
    let transform = argument(c, args, 1);
    let stores = get(c, receiver, STORES)?;
    let map_set = api_member(c, api::MAP_SET)?;
    c.call_rooted(map_set, stores, &[store, transform])?;
    Ok(c.undefined())
}

/// `unbindStore(store)`: whether `store` was bound.
pub(crate) fn channel_unbind_store(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let Some(key) = channel_key(c, receiver)? else {
        return Ok(c.boolean(false));
    };
    let stores = get(c, receiver, STORES)?;
    if !c.is_object_rooted(stores)? {
        return Ok(c.boolean(false));
    }
    let store = argument(c, args, 0);
    let map_delete = api_member(c, api::MAP_DELETE)?;
    let removed = c.call_rooted(map_delete, stores, &[store])?;
    if !c.truthy_rooted(removed)? {
        return Ok(c.boolean(false));
    }
    deactivate_if_idle(c, receiver, borrow_key(&key))?;
    Ok(c.boolean(true))
}

/// `get hasSubscribers`: whether the prototype is `ActiveChannel`'s (the getter's data).
pub(crate) fn channel_has_subscribers_getter(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    c.host_function_data()
}

/// Calls each subscriber of `key` with `message` and `name`, as the list stood when the
/// publish began; a subscriber's throw reaches the uncaught-exception path on the next tick
/// and the rest still run.
fn deliver(c: &mut Context<'_>, key: ChannelKey<'_>, message: RootId, name: RootId) -> Result<(), RootedError> {
    let retained = c.host_mut().shared_state().borrow().diagnostics.subscribers(key).to_vec();
    let subscribers = retained.into_iter().filter_map(|root| c.scope_root(root)).collect::<Vec<_>>();
    let undefined = c.undefined();
    for subscriber in subscribers {
        match c.call_rooted(subscriber, undefined, &[message, name]) {
            Ok(_) => {}
            Err(RootedError { exception: Some(exception), .. }) => queue_uncaught(c, exception)?,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// `publish(message)`: every subscriber receives `message` and the channel's name.
pub(crate) fn channel_publish(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let message = argument(c, args, 0);
    if let Some(key) = channel_key(c, receiver)? {
        let name = get(c, receiver, NAME)?;
        deliver(c, borrow_key(&key), message, name)?;
    }
    Ok(c.undefined())
}

/// `runStores(data, fn, thisArg, ...args)`: `fn` runs inside every store bound now, each
/// entered with its transform of `data` (the last bound outermost, as Node nests them),
/// after `data` is published.
pub(crate) fn channel_run_stores(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let data = argument(c, args, 0);
    let function = argument(c, args, 1);
    let this = argument(c, args, 2);
    let rest = c.array_rooted(args.get(3..).unwrap_or_default())?;
    let stores = store_entries(c, receiver)?;
    let remaining = stores.len() / STORE_ENTRY_WIDTH;
    let stores = c.array_rooted(&stores)?;
    let run = c.record_rooted(&STORE_RUN, &[receiver, data, function, this, rest, stores])?;
    run_stores_from(c, run, remaining)
}

fn element(c: &mut Context<'_>, array: RootId, index: usize) -> Outcome {
    let key = c.number(index as f64);
    c.get_property_rooted(array, key)
}

/// The store run `run` with its first `remaining` stores still to enter.
fn run_stores_from(c: &mut Context<'_>, run: RootId, remaining: usize) -> Outcome {
    let data = get(c, run, "data")?;
    let Some(entered) = remaining.checked_sub(1) else {
        let channel = get(c, run, "channel")?;
        call_method(c, channel, "publish", &[data])?;
        let function = get(c, run, "function")?;
        let this = get(c, run, "receiver")?;
        let rest = get(c, run, "arguments")?;
        let rest = c.array_like_values_rooted(rest)?;
        return c.call_rooted(function, this, &rest);
    };
    let stores = get(c, run, "stores")?;
    let store = element(c, stores, entered * STORE_ENTRY_WIDTH)?;
    let transform = element(c, stores, entered * STORE_ENTRY_WIDTH + 1)?;
    let context = if is_undefined(c, transform) {
        data
    } else {
        let undefined = c.undefined();
        match c.call_rooted(transform, undefined, &[data]) {
            Ok(context) => context,
            Err(RootedError { exception: Some(exception), .. }) => {
                queue_uncaught(c, exception)?;
                return run_stores_from(c, run, entered);
            }
            Err(error) => return Err(error),
        }
    };
    let remaining = c.number(entered as f64);
    let step = c.record_rooted(&STORE_STEP, &[run, remaining])?;
    let next = c.host_function_named_with_data(operation("diagnosticsRunStoresNext"), "", step)?;
    call_method(c, store, "run", &[context, next])
}

/// A store run's continuation, which the entered store calls.
pub(crate) fn run_stores_next(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let step = c.host_function_data()?;
    let run = get(c, step, "run")?;
    let remaining = get(c, step, "remaining")?;
    let remaining = c.rooted_value(remaining).and_then(|value| value.as_number()).unwrap_or_default();
    run_stores_from(c, run, remaining as usize)
}

/// `channel(name)`: the channel named `name`, made on first use.
pub(crate) fn channel(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let name = argument(c, args, 0);
    channel_named(c, name)
}

fn channel_named(c: &mut Context<'_>, name: RootId) -> Outcome {
    let existing = channels_get(c, name)?;
    if c.truthy_rooted(existing)? {
        return Ok(existing);
    }
    if !matches!(c.value_type_rooted(name), Some(ValueType::String | ValueType::Symbol)) {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            c,
            name,
            "channel",
            "one of type string or symbol",
        )?);
    }
    let class = api_member(c, api::CHANNEL_CLASS)?;
    c.construct_rooted(class, class, &[name])
}

/// `subscribe(name, subscription)`: `channel(name).subscribe(subscription)`.
pub(crate) fn subscribe(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let name = argument(c, args, 0);
    let subscription = argument(c, args, 1);
    let channel = channel_named(c, name)?;
    call_method(c, channel, "subscribe", &[subscription])
}

/// `unsubscribe(name, subscription)`: `channel(name).unsubscribe(subscription)`.
pub(crate) fn unsubscribe(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let name = argument(c, args, 0);
    let subscription = argument(c, args, 1);
    let channel = channel_named(c, name)?;
    call_method(c, channel, "unsubscribe", &[subscription])
}

/// `hasSubscribers(name)`: the `hasSubscribers` of the channel named `name`, false with none.
pub(crate) fn has_subscribers(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let name = argument(c, args, 0);
    let channel = channels_get(c, name)?;
    if !c.truthy_rooted(channel)? {
        return Ok(c.boolean(false));
    }
    get(c, channel, "hasSubscribers")
}

/// Whether channel `name` has a subscriber, which a publish delivers its message to.
pub(crate) fn channel_has_subscribers(state: &crate::host::SharedNodeState, name: &str) -> bool {
    !state.diagnostics.subscribers(ChannelKey::Name(name)).is_empty()
}

/// A host publish on channel `name`, as the channel's `publish` delivers it: HTTP, net and
/// the module loader own their timing and payload shape.
pub(crate) fn publish_named(c: &mut Context<'_>, name: &str, message: RootId) -> Result<(), RootedError> {
    if !channel_has_subscribers(&c.host_mut().shared_state().borrow(), name) {
        return Ok(());
    }
    let name_value = c.string_rooted(name);
    deliver(c, ChannelKey::Name(name), message, name_value)
}


/// `tracingChannel(nameOrChannels)`: `new TracingChannel(nameOrChannels)`.
pub(crate) fn tracing_channel(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let class = api_member(c, api::TRACING_CLASS)?;
    let source = argument(c, args, 0);
    c.construct_rooted(class, class, &[source])
}

/// `new TracingChannel(nameOrChannels)`: an event channel per trace event, each read-only:
/// `tracing:<name>:<event>` for a name, the object's own channel otherwise.
pub(crate) fn tracing_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    require_new(c, "TracingChannel")?;
    let source = argument(c, args, 0);
    let prefix = c.string_text(source)?;
    for &event in TRACE_EVENTS {
        let channel = match &prefix {
            Some(prefix) => {
                let name = c.string_rooted(&format!("tracing:{prefix}:{event}"));
                channel_named(c, name)?
            }
            None if c.value_type_rooted(source) == Some(ValueType::Object) && !is_nullish(c, source) => {
                let channel = get(c, source, event)?;
                if !is_channel(c, channel)? {
                    return Err(super::util_shared_vm::argument_type_error(
                        c,
                        channel,
                        &format!("nameOrChannels.{event}"),
                        "Channel",
                    )?);
                }
                channel
            }
            None => {
                return Err(super::util_shared_vm::argument_type_error_with_expected(
                    c,
                    source,
                    "nameOrChannels",
                    "of type string or an instance of TracingChannel or Object",
                )?)
            }
        };
        c.define_named_data_property_rooted(receiver, event, channel, false, false, false)?;
    }
    Ok(c.undefined())
}

/// `get hasSubscribers`: the first event channel's truthy `hasSubscribers`, else the last's.
pub(crate) fn tracing_has_subscribers(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let mut answer = c.undefined();
    for &event in TRACE_EVENTS {
        let channel = get(c, receiver, event)?;
        answer = if is_nullish(c, channel) { c.undefined() } else { get(c, channel, "hasSubscribers")? };
        if c.truthy_rooted(answer)? {
            break;
        }
    }
    Ok(answer)
}

/// The event channels a handler table names, each with its handler.
fn handled_events(c: &mut Context<'_>, receiver: RootId, handlers: RootId) -> Result<Vec<(RootId, RootId)>, RootedError> {
    let mut handled = Vec::new();
    for &event in TRACE_EVENTS {
        let handler = get(c, handlers, event)?;
        if !c.truthy_rooted(handler)? {
            continue;
        }
        let channel = get(c, receiver, event)?;
        if !is_nullish(c, channel) {
            handled.push((channel, handler));
        }
    }
    Ok(handled)
}

/// `subscribe(handlers)`: each handler subscribes to its event's channel.
pub(crate) fn tracing_subscribe(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let handlers = argument(c, args, 0);
    for (channel, handler) in handled_events(c, receiver, handlers)? {
        call_method(c, channel, "subscribe", &[handler])?;
    }
    Ok(c.undefined())
}

/// `unsubscribe(handlers)`: whether every handler was subscribed to its event's channel.
pub(crate) fn tracing_unsubscribe(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let handlers = argument(c, args, 0);
    let mut done = true;
    for (channel, handler) in handled_events(c, receiver, handlers)? {
        let removed = call_method(c, channel, "unsubscribe", &[handler])?;
        done &= c.truthy_rooted(removed)?;
    }
    Ok(c.boolean(done))
}

/// A traced call's frame: `fn`, `thisArg`, the arguments, the context object (a new one for
/// undefined) and the receiver's event channels.
fn trace_frame(c: &mut Context<'_>, receiver: RootId, function: RootId, context: RootId, this: RootId, rest: &[RootId]) -> Outcome {
    let context = if is_undefined(c, context) { c.object_rooted()? } else { context };
    let rest = c.array_rooted(rest)?;
    let mut fields = vec![function, this, rest, context];
    for &event in TRACE_EVENTS {
        fields.push(get(c, receiver, event)?);
    }
    c.record_rooted(&TRACE_FRAME, &fields)
}

/// Whether the tracing channel has a subscriber, read as Node reads `this.hasSubscribers`.
fn traced(c: &mut Context<'_>, receiver: RootId) -> Result<bool, RootedError> {
    let subscribed = get(c, receiver, "hasSubscribers")?;
    c.truthy_rooted(subscribed)
}

fn publish_event(c: &mut Context<'_>, frame: RootId, event: &str) -> Result<(), RootedError> {
    let channel = get(c, frame, event)?;
    let context = get(c, frame, "context")?;
    call_method(c, channel, "publish", &[context])?;
    Ok(())
}

/// `start.runStores(context, body)` for a traced call whose body is `op`.
fn run_traced(c: &mut Context<'_>, frame: RootId, op: &str) -> Outcome {
    let body = c.host_function_named_with_data(operation(op), "", frame)?;
    let start = get(c, frame, "start")?;
    let context = get(c, frame, "context")?;
    call_method(c, start, "runStores", &[context, body])
}

/// `try { call } catch (err) { context.error = err; error.publish(context); throw err }
/// finally { end.publish(context) }`.
fn guarded(c: &mut Context<'_>, frame: RootId, call: impl FnOnce(&mut Context<'_>) -> Outcome) -> Outcome {
    let outcome = match call(c) {
        Err(RootedError { exception: Some(exception), error }) => {
            let reported = report_error(c, frame, exception);
            reported.and(Err(RootedError { exception: Some(exception), error }))
        }
        Err(uncatchable) => return Err(uncatchable),
        outcome => outcome,
    };
    publish_event(c, frame, "end")?;
    outcome
}

/// `context.error = error; error.publish(context)`.
fn report_error(c: &mut Context<'_>, frame: RootId, error: RootId) -> Result<(), RootedError> {
    let context = get(c, frame, "context")?;
    put(c, context, "error", error)?;
    publish_event(c, frame, "error")
}

/// The frame's call: `ReflectApply(fn, thisArg, args)`.
fn apply_frame(c: &mut Context<'_>, frame: RootId) -> Outcome {
    let function = get(c, frame, "function")?;
    let this = get(c, frame, "receiver")?;
    let rest = get(c, frame, "arguments")?;
    let rest = c.array_like_values_rooted(rest)?;
    c.call_rooted(function, this, &rest)
}

/// `traceSync(fn, context = {}, thisArg, ...args)`.
pub(crate) fn trace_sync(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let function = argument(c, args, 0);
    let this = argument(c, args, 2);
    let rest = args.get(3..).unwrap_or_default();
    if !traced(c, receiver)? {
        return c.call_rooted(function, this, rest);
    }
    let context = argument(c, args, 1);
    let frame = trace_frame(c, receiver, function, context, this, rest)?;
    run_traced(c, frame, "diagnosticsTraceSyncBody")
}

/// `traceSync`'s body: the call, its result stored as `context.result`.
pub(crate) fn trace_sync_body(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let frame = c.host_function_data()?;
    guarded(c, frame, |c| {
        let result = apply_frame(c, frame)?;
        let context = get(c, frame, "context")?;
        put(c, context, "result", result)?;
        Ok(result)
    })
}

/// `tracePromise(fn, context = {}, thisArg, ...args)`.
pub(crate) fn trace_promise(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let function = argument(c, args, 0);
    let this = argument(c, args, 2);
    let rest = args.get(3..).unwrap_or_default();
    if !traced(c, receiver)? {
        return c.call_rooted(function, this, rest);
    }
    let context = argument(c, args, 1);
    let frame = trace_frame(c, receiver, function, context, this, rest)?;
    run_traced(c, frame, "diagnosticsTracePromiseBody")
}

/// `tracePromise`'s body: the call's result as a native promise, settled through the
/// asynchronous events.
pub(crate) fn trace_promise_body(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let frame = c.host_function_data()?;
    guarded(c, frame, |c| {
        let mut promise = apply_frame(c, frame)?;
        let constructor = api_member(c, api::PROMISE)?;
        if !c.instance_of_rooted(promise, constructor)? {
            let resolve = api_member(c, api::PROMISE_RESOLVE)?;
            promise = c.call_rooted(resolve, constructor, &[promise])?;
        }
        let fulfilled = c.host_function_named_with_data(operation("diagnosticsTraceResolve"), "resolve", frame)?;
        let rejected = c.host_function_named_with_data(operation("diagnosticsTraceReject"), "reject", frame)?;
        let then = api_member(c, api::PROMISE_THEN)?;
        c.call_rooted(then, promise, &[fulfilled, rejected])
    })
}

fn publish_async_events(c: &mut Context<'_>, frame: RootId) -> Result<(), RootedError> {
    publish_event(c, frame, "asyncStart")?;
    publish_event(c, frame, "asyncEnd")
}

/// A traced promise's fulfilment: `context.result`, then the asynchronous events.
pub(crate) fn trace_resolve(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let frame = c.host_function_data()?;
    let result = argument(c, args, 0);
    let context = get(c, frame, "context")?;
    put(c, context, "result", result)?;
    publish_async_events(c, frame)?;
    Ok(result)
}

/// A traced promise's rejection: the error event, the asynchronous events, and a promise
/// rejected with the same reason.
pub(crate) fn trace_reject(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let frame = c.host_function_data()?;
    let reason = argument(c, args, 0);
    report_error(c, frame, reason)?;
    publish_async_events(c, frame)?;
    let constructor = api_member(c, api::PROMISE)?;
    let reject = api_member(c, api::PROMISE_REJECT)?;
    c.call_rooted(reject, constructor, &[reason])
}

/// `traceCallback(fn, position = -1, context = {}, thisArg, ...args)`: the callback at
/// `position` (from the end when negative) is replaced by one publishing the asynchronous
/// events around it.
pub(crate) fn trace_callback(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let function = argument(c, args, 0);
    let this = argument(c, args, 3);
    let rest = args.get(4..).unwrap_or_default();
    if !traced(c, receiver)? {
        return c.call_rooted(function, this, rest);
    }
    let position = argument(c, args, 1);
    let position = if is_undefined(c, position) { LAST_ARGUMENT } else { c.to_number(position)? };
    let position = if position.is_nan() { 0.0 } else { position.trunc() };
    let index = if position < 0.0 { rest.len() as f64 + position } else { position };
    let index = (index >= 0.0 && index < rest.len() as f64).then_some(index as usize);
    let callback = match index {
        Some(index) => rest[index],
        None => c.undefined(),
    };
    if !c.is_callable_rooted(callback)? {
        return Err(super::util_shared_vm::argument_type_error(c, callback, "callback", "function")?);
    }
    let index = index.expect("a callable callback is an argument");
    let context = argument(c, args, 2);
    let mut rest = rest.to_vec();
    let frame = trace_frame(c, receiver, function, context, this, &[])?;
    let wrapper_data = c.record_rooted(&WRAPPED_CALLBACK, &[frame, callback])?;
    rest[index] = c.host_function_named_with_data(operation("diagnosticsTraceWrappedCallback"), "wrappedCallback", wrapper_data)?;
    let rest = c.array_rooted(&rest)?;
    put(c, frame, "arguments", rest)?;
    run_traced(c, frame, "diagnosticsTraceCallbackBody")
}

/// `traceCallback`'s body: the call, with the wrapped callback.
pub(crate) fn trace_callback_body(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let frame = c.host_function_data()?;
    guarded(c, frame, |c| apply_frame(c, frame))
}

/// `wrappedCallback(err, res)`: the error event or `context.result`, then the callback inside
/// `asyncStart.runStores`, `asyncEnd` published however it completes.
pub(crate) fn trace_wrapped_callback(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let frame = get(c, data, "frame")?;
    let callback = get(c, data, "callback")?;
    let error = argument(c, args, 0);
    if c.truthy_rooted(error)? {
        report_error(c, frame, error)?;
    } else {
        let result = argument(c, args, 1);
        let context = get(c, frame, "context")?;
        put(c, context, "result", result)?;
    }
    let arguments = c.array_rooted(args)?;
    let call = c.record_rooted(&CALLBACK_CALL, &[frame, callback, receiver, arguments])?;
    let body = c.host_function_named_with_data(operation("diagnosticsTraceCallbackContinue"), "", call)?;
    let async_start = get(c, frame, "asyncStart")?;
    let context = get(c, frame, "context")?;
    call_method(c, async_start, "runStores", &[context, body])?;
    Ok(c.undefined())
}

/// The traced callback's own call, `asyncEnd` published however it completes.
pub(crate) fn trace_callback_continue(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let call = c.host_function_data()?;
    let callback = get(c, call, "callback")?;
    let this = get(c, call, "receiver")?;
    let arguments = get(c, call, "arguments")?;
    let arguments = c.array_like_values_rooted(arguments)?;
    let outcome = c.call_rooted(callback, this, &arguments);
    let frame = get(c, call, "frame")?;
    publish_event(c, frame, "asyncEnd")?;
    outcome
}

crate::host::startup_snapshot::host_snap_struct!(SharedDiagnosticsState { channels, identified, next_ordinal, api, module });
