//! Rust-owned diagnostics channel state and mechanical API surface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_DIAGNOSTICS_BOUNDED_CHANNEL, SPEC_DIAGNOSTICS_BOUNDED_RUN,
    SPEC_DIAGNOSTICS_BOUNDED_SUBSCRIBE, SPEC_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE,
    SPEC_DIAGNOSTICS_CHANNEL, SPEC_DIAGNOSTICS_CHANNEL_BIND_STORE,
    SPEC_DIAGNOSTICS_CHANNEL_CONSTRUCTOR, SPEC_DIAGNOSTICS_CHANNEL_PUBLISH,
    SPEC_DIAGNOSTICS_CHANNEL_SUBSCRIBE, SPEC_DIAGNOSTICS_CHANNEL_UNBIND_STORE,
    SPEC_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE, SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS,
    SPEC_DIAGNOSTICS_SUBSCRIBE, SPEC_DIAGNOSTICS_TRACING_CHANNEL,
    SPEC_DIAGNOSTICS_TRACING_SUBSCRIBE, SPEC_DIAGNOSTICS_TRACING_TRACE_SYNC,
    SPEC_DIAGNOSTICS_TRACING_UNSUBSCRIBE, SPEC_DIAGNOSTICS_UNSUBSCRIBE,
};

const ID: &str = "\0quench:diagnostics_channel:id";
const NAME: &str = "\0quench:diagnostics_channel:name";
const TRACE: &str = "\0quench:diagnostics_channel:tracing";
const BOUNDED: &str = "\0quench:diagnostics_channel:bounded";
const TRACE_CHANNELS: [&str; 5] = ["start", "end", "asyncStart", "asyncEnd", "error"];

thread_local! { static CHANNEL_PROTO: RefCell<Option<Value>> = const { RefCell::new(None) }; }

struct ChannelData {
    name: Value,
    subscribers: Vec<Value>,
    store: Value,
}

impl ChannelData {
    fn new(name: Value) -> Self {
        Self {
            name,
            subscribers: Vec::new(),
            store: Value::Undefined,
        }
    }
}

pub struct DiagnosticsState {
    next_id: u64,
    channels: HashMap<String, (u64, Rc<RefCell<ChannelData>>)>,
    by_id: HashMap<u64, Rc<RefCell<ChannelData>>>,
}

impl DiagnosticsState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            channels: HashMap::new(),
            by_id: HashMap::new(),
        }
    }
}

pub fn build() -> Value {
    let prototype = host_api::object(Vec::new());
    CHANNEL_PROTO.with(|slot| *slot.borrow_mut() = Some(prototype.clone()));
    let mut constructor = crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_CONSTRUCTOR);
    constructor = execute::set_property(constructor, "prototype", prototype.clone());
    crate::host::namespace_object(vec![
        ("Channel", constructor),
        (
            "BoundedChannel",
            crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_CHANNEL),
        ),
        ("channel", crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL)),
        (
            "subscribe",
            crate::host::capability(SPEC_DIAGNOSTICS_SUBSCRIBE),
        ),
        (
            "unsubscribe",
            crate::host::capability(SPEC_DIAGNOSTICS_UNSUBSCRIBE),
        ),
        (
            "hasSubscribers",
            crate::host::capability(SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS),
        ),
        ("channelNames", Value::Undefined),
        (
            "tracingChannel",
            crate::host::capability(SPEC_DIAGNOSTICS_TRACING_CHANNEL),
        ),
        (
            "boundedChannel",
            crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_CHANNEL),
        ),
    ])
    .unwrap_or(Value::Undefined)
}

pub fn channel(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = args.first().cloned().unwrap_or(Value::Undefined);
    let key = channel_key(&name)?;
    if let Some((id, _)) = state.borrow().diagnostics.channels.get(&key) {
        return Ok(channel_object(*id, name));
    }
    let mut host = state.borrow_mut();
    let id = host.diagnostics.next_id;
    host.diagnostics.next_id += 1;
    let data = Rc::new(RefCell::new(ChannelData::new(name.clone())));
    host.diagnostics.channels.insert(key, (id, data.clone()));
    host.diagnostics.by_id.insert(id, data);
    Ok(channel_object(id, name))
}

pub fn new_channel(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    channel(state, None, args)
}

pub fn tracing_channel(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = args.first().ok_or_else(|| type_error("nameOrChannels"))?;
    let channels = TRACE_CHANNELS
        .iter()
        .map(|name| tracing_member(state, source, name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tracing_object(channels))
}

pub fn bounded_channel(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = args.first().cloned().unwrap_or(Value::Undefined);
    let (start, end) = if let Value::String(name) = source {
        let prefix = format!("tracing:{name}");
        (
            channel(state, None, &[Value::String(format!("{prefix}:start"))])?,
            channel(state, None, &[Value::String(format!("{prefix}:end"))])?,
        )
    } else {
        (
            execute::get_property(&source, "start"),
            execute::get_property(&source, "end"),
        )
    };
    let object = host_api::object(vec![
        (BOUNDED.into(), Value::Boolean(true)),
        ("start".into(), start),
        ("end".into(), end),
        (
            "subscribe".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_SUBSCRIBE),
        ),
        (
            "unsubscribe".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE),
        ),
        (
            "run".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_RUN),
        ),
    ]);
    Ok(object)
}

pub fn bounded_subscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("boundedChannel"))?;
    let handlers = args.first().cloned().unwrap_or(Value::Undefined);
    for name in ["start", "end"] {
        let callback = execute::get_property(&handlers, name);
        if !matches!(callback, Value::Undefined) {
            subscribe_to(state, &execute::get_property(receiver, name), &callback)?;
        }
    }
    Ok(Value::Undefined)
}

pub fn bounded_unsubscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("boundedChannel"))?;
    let handlers = args.first().cloned().unwrap_or(Value::Undefined);
    let mut removed = true;
    for name in ["start", "end"] {
        let callback = execute::get_property(&handlers, name);
        if !matches!(callback, Value::Undefined) {
            removed &= matches!(
                unsubscribe_from(state, &execute::get_property(receiver, name), &callback)?,
                Value::Boolean(true)
            );
        }
    }
    Ok(Value::Boolean(removed))
}

pub fn bounded_run(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("boundedChannel"))?;
    let context = args.first().cloned().unwrap_or(Value::Undefined);
    let callback = args.get(1).ok_or_else(|| type_error("fn"))?;
    if !quench_runtime::is_callable(callback) {
        return Err(type_error("fn"));
    }
    let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
    let call_args = args.get(3..).unwrap_or(&[]);
    let start = execute::get_property(receiver, "start");
    let end = execute::get_property(receiver, "end");
    if channel_has_subscribers(state, &start) {
        publish(state, Some(&start), std::slice::from_ref(&context))?;
    }
    let result = execute::call(callback, &this_arg, call_args);
    if channel_has_subscribers(state, &end) {
        publish(state, Some(&end), std::slice::from_ref(&context))?;
    }
    result
}

fn tracing_member(
    state: &Rc<RefCell<HostState>>,
    source: &Value,
    name: &str,
) -> Result<Value, VmError> {
    if let Value::String(base) = source {
        return channel(
            state,
            None,
            &[Value::String(format!("tracing:{base}:{name}"))],
        );
    }
    let value = execute::get_property_result(source, name).unwrap_or(Value::Undefined);
    if matches!(value, Value::Undefined) {
        return Ok(Value::Undefined);
    }
    if execute::get_property_result(&value, ID).is_err() {
        return Err(type_error("nameOrChannels"));
    }
    Ok(value)
}

fn tracing_object(channels: Vec<Value>) -> Value {
    let mut properties = vec![(TRACE.into(), Value::Boolean(true))];
    for (name, channel) in TRACE_CHANNELS.iter().zip(channels) {
        properties.push(((*name).into(), channel));
    }
    properties.extend([
        (
            "subscribe".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_TRACING_SUBSCRIBE),
        ),
        (
            "unsubscribe".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_TRACING_UNSUBSCRIBE),
        ),
        (
            "traceSync".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_TRACING_TRACE_SYNC),
        ),
    ]);
    let object = host_api::object(properties);
    let descriptor = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS),
        ),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    execute::define_property(object, "hasSubscribers", descriptor).unwrap_or(Value::Undefined)
}

pub fn tracing_subscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("tracingChannel"))?;
    let handlers = args.first().cloned().unwrap_or(Value::Undefined);
    for name in TRACE_CHANNELS {
        let callback = execute::get_property_result(&handlers, name).unwrap_or(Value::Undefined);
        if !matches!(callback, Value::Undefined) {
            let channel = execute::get_property(receiver, name);
            subscribe_to(state, &channel, &callback)?;
        }
    }
    Ok(Value::Undefined)
}

pub fn tracing_unsubscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("tracingChannel"))?;
    let handlers = args.first().cloned().unwrap_or(Value::Undefined);
    let mut removed = true;
    for name in TRACE_CHANNELS {
        let callback = execute::get_property_result(&handlers, name).unwrap_or(Value::Undefined);
        if !matches!(callback, Value::Undefined) {
            let channel = execute::get_property(receiver, name);
            removed &= matches!(
                unsubscribe_from(state, &channel, &callback)?,
                Value::Boolean(true)
            );
        }
    }
    Ok(Value::Boolean(removed))
}

pub fn trace_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("tracingChannel"))?;
    let callback = args.first().ok_or_else(|| type_error("fn"))?;
    if !quench_runtime::is_callable(callback) {
        return Err(type_error("fn"));
    }
    let mut context = args
        .get(1)
        .filter(|value| execute::is_truthy(value))
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
    let call_args = args.get(3..).unwrap_or(&[]);
    let start = execute::get_property(receiver, "start");
    let end = execute::get_property(receiver, "end");
    let error = execute::get_property(receiver, "error");
    if channel_has_subscribers(state, &start) {
        publish(state, Some(&start), std::slice::from_ref(&context))?;
    }
    match execute::call(callback, &this_arg, call_args) {
        Ok(result) => {
            context = execute::set_property(context, "result", result.clone());
            if channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&context))?;
            }
            Ok(result)
        }
        Err(thrown) => {
            let error_value = match &thrown {
                VmError::Thrown(value) => value.clone(),
                _ => Value::Undefined,
            };
            context = execute::set_property(context, "error", error_value);
            if channel_has_subscribers(state, &error) {
                publish(state, Some(&error), std::slice::from_ref(&context))?;
            }
            if channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&context))?;
            }
            Err(thrown)
        }
    }
}

fn channel_object(id: u64, name: Value) -> Value {
    let mut properties = vec![
        (ID.into(), Value::Number(id as f64)),
        (NAME.into(), name.clone()),
        ("name".into(), name),
        (
            "subscribe".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_SUBSCRIBE),
        ),
        (
            "unsubscribe".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE),
        ),
        (
            "publish".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_PUBLISH),
        ),
        (
            "bindStore".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_BIND_STORE),
        ),
        (
            "unbindStore".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_UNBIND_STORE),
        ),
    ];
    CHANNEL_PROTO.with(|slot| {
        if let Some(prototype) = slot.borrow().clone() {
            properties.push(("\0prototype".into(), prototype));
        }
    });
    let object = host_api::object(properties);
    let descriptor = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS),
        ),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    execute::define_property(object, "hasSubscribers", descriptor).unwrap_or(Value::Undefined)
}

pub fn subscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (name, callback) = static_channel_args(receiver, args)?;
    let channel = channel(state, None, &[name])?;
    subscribe_to(state, &channel, &callback)?;
    Ok(Value::Undefined)
}

pub fn unsubscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (name, callback) = static_channel_args(receiver, args)?;
    let channel = channel(state, None, &[name])?;
    unsubscribe_from(state, &channel, &callback)
}

pub fn has_subscribers(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if receiver.is_some_and(|value| {
        TRACE_CHANNELS.iter().all(|name| {
            let channel = execute::get_property(value, name);
            channel_data(state, &channel).is_ok()
        })
    }) {
        let tracing = receiver.unwrap();
        return Ok(Value::Boolean(TRACE_CHANNELS.iter().any(|name| {
            let channel = execute::get_property(tracing, name);
            channel_data(state, &channel)
                .map(|data| !data.borrow().subscribers.is_empty())
                .unwrap_or(false)
        })));
    }
    let channel = match receiver {
        Some(value)
            if matches!(
                execute::get_property_result(value, ID),
                Ok(Value::Number(_))
            ) =>
        {
            value.clone()
        }
        Some(value) if args.is_empty() => channel(state, None, std::slice::from_ref(value))?,
        _ => channel(state, None, &args[..1.min(args.len())])?,
    };
    Ok(Value::Boolean(
        channel_data(state, &channel)?.borrow().subscribers.len() > 0,
    ))
}

pub fn channel_subscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let callback = args.first().ok_or_else(|| type_error("subscriber"))?;
    subscribe_to(state, receiver, callback)?;
    Ok(receiver.clone())
}

pub fn channel_unsubscribe(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let callback = args.first().ok_or_else(|| type_error("subscriber"))?;
    unsubscribe_from(state, receiver, callback)
}

pub fn publish(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let data = channel_data(state, receiver)?;
    let message = args.first().cloned().unwrap_or(Value::Undefined);
    let name = data.borrow().name.clone();
    let callbacks = data.borrow().subscribers.clone();
    for callback in callbacks {
        execute::call(
            &callback,
            &Value::Undefined,
            &[message.clone(), name.clone()],
        )?;
    }
    Ok(Value::Undefined)
}

pub fn bind_store(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let data = channel_data(state, receiver.ok_or_else(|| type_error("channel"))?)?;
    data.borrow_mut().store = args.first().cloned().unwrap_or(Value::Undefined);
    Ok(receiver.unwrap().clone())
}

pub fn unbind_store(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let data = channel_data(state, receiver)?;
    if args
        .first()
        .is_none_or(|value| *value == data.borrow().store)
    {
        data.borrow_mut().store = Value::Undefined;
    }
    Ok(receiver.clone())
}

fn subscribe_to(
    state: &Rc<RefCell<HostState>>,
    channel: &Value,
    callback: &Value,
) -> Result<(), VmError> {
    if !quench_runtime::is_callable(callback) {
        return Err(type_error("subscriber"));
    }
    let data = channel_data(state, channel)?;
    let mut data = data.borrow_mut();
    if !data.subscribers.iter().any(|value| value == callback) {
        data.subscribers.push(callback.clone());
    }
    Ok(())
}

fn unsubscribe_from(
    state: &Rc<RefCell<HostState>>,
    channel: &Value,
    callback: &Value,
) -> Result<Value, VmError> {
    let data = channel_data(state, channel)?;
    let mut data = data.borrow_mut();
    if let Some(index) = data.subscribers.iter().position(|value| value == callback) {
        data.subscribers.remove(index);
        Ok(Value::Boolean(true))
    } else {
        Ok(Value::Boolean(false))
    }
}

fn channel_data(
    state: &Rc<RefCell<HostState>>,
    channel: &Value,
) -> Result<Rc<RefCell<ChannelData>>, VmError> {
    let id = execute::get_property_result(channel, ID)
        .ok()
        .and_then(|value| match value {
            Value::Number(n) => Some(n as u64),
            _ => None,
        })
        .ok_or_else(|| type_error("channel"))?;
    state
        .borrow()
        .diagnostics
        .by_id
        .get(&id)
        .cloned()
        .ok_or_else(|| type_error("channel"))
}

fn channel_has_subscribers(state: &Rc<RefCell<HostState>>, channel: &Value) -> bool {
    channel_data(state, channel)
        .map(|data| !data.borrow().subscribers.is_empty())
        .unwrap_or(false)
}

fn channel_key(value: &Value) -> Result<String, VmError> {
    match value {
        Value::String(name) if !name.contains('\0') || name.starts_with("Symbol.") => {
            Ok(name.clone())
        }
        _ => Err(type_error("channel")),
    }
}

fn static_channel_args(
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<(Value, Value), VmError> {
    if args.len() >= 2 {
        return Ok((args[0].clone(), args[1].clone()));
    }
    match (receiver, args.first()) {
        (Some(name), Some(callback)) => Ok((name.clone(), callback.clone())),
        _ => Err(type_error("channel")),
    }
}

fn type_error(argument: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"{argument}\" argument must be of type function"
            )),
        ),
    ]))
}
