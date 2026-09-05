//! Rust-owned diagnostics channel state and mechanical API surface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::{PromiseState, Value};

use crate::host::HostState;
use crate::registry::{
    SPEC_DIAGNOSTICS_BOUNDED_CHANNEL, SPEC_DIAGNOSTICS_BOUNDED_RUN, SPEC_DIAGNOSTICS_BOUNDED_SCOPE,
    SPEC_DIAGNOSTICS_BOUNDED_SUBSCRIBE, SPEC_DIAGNOSTICS_BOUNDED_UNSUBSCRIBE,
    SPEC_DIAGNOSTICS_CHANNEL, SPEC_DIAGNOSTICS_CHANNEL_BIND_STORE,
    SPEC_DIAGNOSTICS_CHANNEL_CONSTRUCTOR, SPEC_DIAGNOSTICS_CHANNEL_PUBLISH,
    SPEC_DIAGNOSTICS_CHANNEL_RUN_STORES, SPEC_DIAGNOSTICS_CHANNEL_SCOPE,
    SPEC_DIAGNOSTICS_CHANNEL_SUBSCRIBE, SPEC_DIAGNOSTICS_CHANNEL_UNBIND_STORE,
    SPEC_DIAGNOSTICS_CHANNEL_UNSUBSCRIBE, SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS,
    SPEC_DIAGNOSTICS_SCOPE_DISPOSE, SPEC_DIAGNOSTICS_SUBSCRIBE, SPEC_DIAGNOSTICS_TRACING_CHANNEL,
    SPEC_DIAGNOSTICS_TRACING_SUBSCRIBE, SPEC_DIAGNOSTICS_TRACING_TRACE_SYNC,
    SPEC_DIAGNOSTICS_TRACING_UNSUBSCRIBE, SPEC_DIAGNOSTICS_UNSUBSCRIBE,
};

const ID: &str = "\0quench:diagnostics_channel:id";
const NAME: &str = "\0quench:diagnostics_channel:name";
const TRACE: &str = "\0quench:diagnostics_channel:tracing";
const BOUNDED: &str = "\0quench:diagnostics_channel:bounded";
const SCOPE_STORE: &str = "\0quench:diagnostics_channel:scope:store";
const SCOPE_PREVIOUS: &str = "\0quench:diagnostics_channel:scope:previous";
const SCOPE_ACTIVE: &str = "\0quench:diagnostics_channel:scope:active";
const SCOPE_PUBLISHED: &str = "\0quench:diagnostics_channel:scope:published";
const SCOPE_END: &str = "\0quench:diagnostics_channel:scope:end";
const SCOPE_CONTEXT: &str = "\0quench:diagnostics_channel:scope:context";
const TRACE_CHANNELS: [&str; 5] = ["start", "end", "asyncStart", "asyncEnd", "error"];

thread_local! {
    static CHANNEL_PROTO: RefCell<Option<Value>> = const { RefCell::new(None) };
    static BOUNDED_PROTO: RefCell<Option<Value>> = const { RefCell::new(None) };
    static TEST_ROOT_TRACE_EMITTED: RefCell<bool> = const { RefCell::new(false) };
}

struct ChannelData {
    name: Value,
    subscribers: Vec<Value>,
    stores: Vec<(Value, Value)>,
}

impl ChannelData {
    fn new(name: Value) -> Self {
        Self {
            name,
            subscribers: Vec::new(),
            stores: Vec::new(),
        }
    }
}

pub struct DiagnosticsState {
    next_id: u64,
    channels: HashMap<String, (u64, Rc<RefCell<ChannelData>>, Value)>,
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
    let bounded_prototype = host_api::object(Vec::new());
    BOUNDED_PROTO.with(|slot| *slot.borrow_mut() = Some(bounded_prototype.clone()));
    let mut constructor = crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_CONSTRUCTOR);
    constructor = execute::set_property(constructor, "prototype", prototype.clone());
    let bounded_constructor = execute::set_property(
        crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_CHANNEL),
        "prototype",
        bounded_prototype,
    );
    crate::host::namespace_object(vec![
        ("Channel", constructor),
        ("BoundedChannel", bounded_constructor),
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
    // End the immutable lookup borrow before acquiring the mutable host
    // borrow below. Cluster workers can request the same channel while an
    // event callback is still on the stack, so relying on the `if let`
    // temporary's lifetime here triggers a RefCell re-entrant-borrow panic.
    let existing = {
        let host = state.borrow();
        host.diagnostics
            .channels
            .get(&key)
            .map(|(_, _, object)| object.clone())
    };
    if let Some(object) = existing {
        return Ok(object);
    }
    let mut host = state.borrow_mut();
    let id = host.diagnostics.next_id;
    host.diagnostics.next_id += 1;
    let data = Rc::new(RefCell::new(ChannelData::new(name.clone())));
    let object = channel_object(id, name);
    host.diagnostics
        .channels
        .insert(key, (id, data.clone(), object.clone()));
    host.diagnostics.by_id.insert(id, data);
    Ok(object)
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
    if !matches!(source, Value::String(_) | Value::Object(_)) {
        return Err(tracing_type_error());
    }
    if matches!(source, Value::Object(_))
        && matches!(execute::get_property(source, "start"), Value::Undefined)
    {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "message".into(),
                Value::String("Cannot convert undefined or null to object".into()),
            ),
        ])));
    }
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
        (
            "withScope".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_BOUNDED_SCOPE),
        ),
    ]);
    let descriptor = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_HAS_SUBSCRIBERS),
        ),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    let object =
        execute::define_property(object, "hasSubscribers", descriptor).unwrap_or(Value::Undefined);
    Ok(BOUNDED_PROTO.with(|slot| {
        slot.borrow()
            .clone()
            .map(|prototype| execute::set_property(object.clone(), "\0prototype", prototype))
            .unwrap_or(object)
    }))
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
    let stores = channel_stores(state, &start);
    let (previous, transform_errors) = enter_stores_with_errors(&stores, &context);
    let start_result = if channel_has_subscribers(state, &start) {
        publish(state, Some(&start), std::slice::from_ref(&context))
    } else {
        Ok(Value::Undefined)
    };
    if let Err(error) = start_result {
        restore_stores(Some(&previous));
        for error in transform_errors {
            schedule_uncaught(state, error)?;
        }
        return Err(error);
    }
    let result = execute::call(callback, &this_arg, call_args);
    let end_result = if channel_has_subscribers(state, &end) {
        publish(state, Some(&end), std::slice::from_ref(&context))
    } else {
        Ok(Value::Undefined)
    };
    restore_stores(Some(&previous));
    for error in transform_errors {
        schedule_uncaught(state, error)?;
    }
    end_result.and(result)
}

pub fn bounded_scope(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("boundedChannel"))?;
    let context = args.first().cloned().unwrap_or(Value::Undefined);
    let start = execute::get_property(receiver, "start");
    let end = execute::get_property(receiver, "end");
    let stores = channel_stores(state, &start);
    let (previous, transform_errors) = enter_stores_with_errors(&stores, &context);
    if channel_has_subscribers(state, &start) {
        publish(state, Some(&start), std::slice::from_ref(&context))?;
    }
    let scope = host_api::object(vec![
        (
            SCOPE_STORE.into(),
            host_api::array(previous.iter().map(|(store, _)| store.clone()).collect()),
        ),
        (
            SCOPE_PREVIOUS.into(),
            host_api::array(previous.iter().map(|(_, value)| value.clone()).collect()),
        ),
        (SCOPE_ACTIVE.into(), Value::Boolean(true)),
        (SCOPE_END.into(), end),
        (SCOPE_CONTEXT.into(), context),
        (
            "dispose".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_SCOPE_DISPOSE),
        ),
        (
            "Symbol.dispose".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_SCOPE_DISPOSE),
        ),
    ]);
    for error in transform_errors {
        schedule_uncaught(state, error)?;
    }
    Ok(scope)
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
    if channel_data(state, &value).is_err() {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The \"nameOrChannels.{name}\" property must be an instance of Channel"
                )),
            ),
        ])));
    }
    Ok(value)
}

/// Instrument the synchronous CJS loader using the same tracing-channel data
/// model as user-facing `tracingChannel("module.require")` subscribers.
pub(crate) fn module_require_start(
    state: &Rc<RefCell<HostState>>,
    parent: String,
    id: String,
) -> Result<Option<Value>, VmError> {
    let start = channel(
        state,
        None,
        &[Value::String("tracing:module.require:start".into())],
    )?;
    if !channel_has_subscribers(state, &start) {
        return Ok(None);
    }
    let event = host_api::object(vec![
        ("parentFilename".into(), Value::String(parent)),
        ("id".into(), Value::String(id)),
    ]);
    publish(state, Some(&start), std::slice::from_ref(&event))?;
    Ok(Some(event))
}

pub(crate) fn module_require_end(
    state: &Rc<RefCell<HostState>>,
    event: Value,
    result: &Result<Value, VmError>,
) -> Result<(), VmError> {
    let channel_name = match result {
        Ok(value) => {
            let _ = execute::set_property_in_place(&event, "result", value.clone());
            None
        }
        Err(VmError::Thrown(error)) => {
            let _ = execute::set_property_in_place(&event, "error", error.clone());
            Some("tracing:module.require:error")
        }
        Err(_) => Some("tracing:module.require:error"),
    };
    if let Some(channel_name) = channel_name {
        let channel = channel(state, None, &[Value::String(channel_name.into())])?;
        if channel_has_subscribers(state, &channel) {
            publish(state, Some(&channel), std::slice::from_ref(&event))?;
        }
    }
    let end = channel(
        state,
        None,
        &[Value::String("tracing:module.require:end".into())],
    )?;
    if channel_has_subscribers(state, &end) {
        publish(state, Some(&end), std::slice::from_ref(&event))?;
    }
    Ok(())
}

/// Trace the host's dynamic `import()` boundary.  Dynamic imports are lowered
/// to a Rust resolver in this runtime, so the loader must publish the same
/// stable event object that Node publishes around its ESM resolver.  Keep the
/// event lifecycle here, next to the tracing-channel implementation, rather
/// than teaching each resolver call site about channel ordering.
pub fn module_import_begin(
    state: &Rc<RefCell<HostState>>,
    parent: String,
    url: String,
) -> Result<Option<Value>, VmError> {
    let channels = TRACE_CHANNELS
        .iter()
        .map(|name| channel(state, None, &[Value::String(format!("tracing:module.import:{name}"))]))
        .collect::<Result<Vec<_>, _>>()?;
    if !channels.iter().any(|value| channel_has_subscribers(state, value)) {
        return Ok(None);
    }
    let event = host_api::object(vec![
        ("parentURL".into(), Value::String(parent)),
        ("url".into(), Value::String(url)),
    ]);
    if channel_has_subscribers(state, &channels[0]) {
        publish(state, Some(&channels[0]), std::slice::from_ref(&event))?;
    }
    Ok(Some(event))
}

pub fn module_import_parent_url(state: &Rc<RefCell<HostState>>) -> String {
    let parent = state
        .borrow()
        .module_stack
        .last()
        .cloned()
        .or_else(|| quench_runtime::vm::current_context().source_name().map(str::to_owned))
        .unwrap_or_default();
    if parent.starts_with("file://") {
        return parent;
    }
    crate::modules::url_file::path_to_file_url(
        state,
        None,
        &[Value::String(parent.clone())],
    )
    .ok()
    .and_then(|url| execute::to_js_string(&execute::get_property(&url, "href")).ok())
    .unwrap_or(parent)
}

pub fn module_import_end(
    state: &Rc<RefCell<HostState>>,
    event: Value,
    result: Result<Value, Value>,
) -> Result<(), VmError> {
    let channels = TRACE_CHANNELS
        .iter()
        .map(|name| channel(state, None, &[Value::String(format!("tracing:module.import:{name}"))]))
        .collect::<Result<Vec<_>, _>>()?;
    let (value, error) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    let failed = error.is_some();
    // Node's import trace closes the synchronous span before publishing the
    // error (if any), then reports the asynchronous promise transition.
    if channel_has_subscribers(state, &channels[1]) {
        publish(state, Some(&channels[1]), std::slice::from_ref(&event))?;
    }
    if let Some(error) = error {
        execute::set_property_in_place(&event, "error", error);
    } else if let Some(value) = value {
        execute::set_property_in_place(&event, "result", value);
    }
    if failed && channel_has_subscribers(state, &channels[4]) {
        publish(state, Some(&channels[4]), std::slice::from_ref(&event))?;
    }
    if channel_has_subscribers(state, &channels[2]) {
        publish(state, Some(&channels[2]), std::slice::from_ref(&event))?;
    }
    if channel_has_subscribers(state, &channels[3]) {
        publish(state, Some(&channels[3]), std::slice::from_ref(&event))?;
    }
    Ok(())
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
        (
            "traceCallback".into(),
            eval_function(
                "function(fn, position, context, thisArg) {\
                  var args = Array.prototype.slice.call(arguments, 4);\
                  var index = position < 0 ? args.length + position : position;\
                  var callback = args[index];\
                  if (typeof fn !== 'function' || typeof callback !== 'function') throw Object.assign(new TypeError('The \\\"callback\\\" argument must be of type function'), { code: 'ERR_INVALID_ARG_TYPE' });\
                  if (!this.hasSubscribers) return fn.apply(thisArg, args);\
                  var startScope = this.start?.withStoreScope(context); this.start?.publish(context);\
                  var self = this;\
                  var done = function(error, result) {\
                    if (error) { context.error = error; self.error?.publish(context); }\
                    else context.result = result;\
                    var asyncScope = self.asyncStart?.withStoreScope(context); self.asyncStart?.publish(context); self.asyncEnd?.publish(context);\
                    try { return callback(error, result); } finally { asyncScope?.dispose?.(); }\
                  };\
                  args[index] = done;\
                  try { fn.apply(thisArg, args); self.end?.publish(context); }\
                  catch (error) { context.error = error; self.error?.publish(context); self.end?.publish(context); throw error; }\
                  finally { startScope?.dispose?.(); }\
                }",
            )
            .unwrap_or(Value::Undefined),
        ),
        (
            "tracePromise".into(),
            eval_function(
                "function(fn, context, thisArg) {\
                  var args = Array.prototype.slice.call(arguments, 3);\
                  context = context || {};\
                  if (typeof fn !== 'function') throw new TypeError('fn');\
                  if (!this.hasSubscribers) return fn.apply(thisArg, args);\
                  var self = this; var startScope = this.start?.withStoreScope(context); if (!startScope || startScope[\"\\0quench:diagnostics_channel:scope:published\"] !== true) this.start?.publish(context);\
                  var settle = function(error, value) {\
                    Object.defineProperty(context, error ? 'error' : 'result', { value: error || value, configurable: true, writable: true });\
                    if (error) self.error?.publish(context);\
                    var asyncScope = self.asyncStart?.withStoreScope(context); if (!asyncScope || asyncScope[\"\\0quench:diagnostics_channel:scope:published\"] !== true) self.asyncStart?.publish(context); self.asyncEnd?.publish(context); asyncScope?.dispose?.();\
                    self.end?.publish(context); if (startScope) startScope.dispose();\
                    if (error) throw error; return value;\
                  };\
                  var result;\
                  try { result = fn.apply(thisArg, args); }\
                  catch (error) { if (startScope) startScope.dispose(); settle(error); throw error; }\
                  if (!result || typeof result.then !== 'function') { process.emitWarning(\"tracePromise was called with the function '<anonymous>', which returned a non-thenable.\"); Object.defineProperty(context, 'result', { value: result, configurable: true, writable: true }); self.end?.publish(context); if (startScope) startScope.dispose(); return result; }\
                  if (startScope) startScope.dispose();\
                  if (result instanceof Promise) { Object.defineProperty(result, \"\\0quench:diagnostics:trace-promise-clear-store\", { value: { channel: self, context: context }, configurable: true }); return result; }\
                  result.then(function(value) { settle(null, value); }, function(error) { settle(error); throw error; });\
                  return result;\
                }",
            )
            .unwrap_or(Value::Undefined),
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
    let tracing = TRACE_CHANNELS
        .iter()
        .any(|name| channel_has_subscribers(state, &execute::get_property(receiver, name)));
    let start_store = channel_store(state, &start);
    let previous_store = enter_store(start_store.as_ref(), &context);
    if tracing && channel_has_subscribers(state, &start) {
        publish(state, Some(&start), std::slice::from_ref(&context))?;
    }
    match execute::call(callback, &this_arg, call_args) {
        Ok(result) => {
            execute::set_property_in_place(&context, "result", result.clone());
            let completion = context.clone();
            if tracing && channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&completion))?;
            }
            restore_store(start_store.as_ref(), previous_store);
            Ok(result)
        }
        Err(thrown) => {
            let error_value = match &thrown {
                VmError::Thrown(value) => value.clone(),
                _ => Value::Undefined,
            };
            execute::set_property_in_place(&context, "error", error_value);
            let completion = context.clone();
            if tracing && channel_has_subscribers(state, &error) {
                publish(state, Some(&error), std::slice::from_ref(&completion))?;
            }
            if tracing && channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&completion))?;
            }
            restore_store(start_store.as_ref(), previous_store);
            Err(thrown)
        }
    }
}

/// Complete a native Promise trace from the engine's promise-resolution edge.
/// The original Promise is returned unchanged; this host hook supplies the
/// async/error/end events without attaching a rejection handler that would
/// alter unhandled-rejection semantics.
pub(crate) fn tracing_promise_settle(
    state: &Rc<RefCell<HostState>>,
    channel: &Value,
    context: &Value,
    promise: &quench_runtime::value::PromiseData,
) -> Result<(), VmError> {
    let settled = promise.state.borrow().clone();
    let (error, result) = match settled {
        PromiseState::Rejected(error) => (Some(error), None),
        PromiseState::Fulfilled(result) => (None, Some(result)),
        PromiseState::Pending => return Ok(()),
    };
    let key = error.as_ref().map(|_| "error").unwrap_or("result");
    let value = error.clone().or_else(|| result.clone()).unwrap_or(Value::Undefined);
    let descriptor = host_api::object(vec![
        ("value".into(), value),
        ("configurable".into(), Value::Boolean(true)),
        ("enumerable".into(), Value::Boolean(false)),
        ("writable".into(), Value::Boolean(true)),
    ]);
    execute::define_property(context.clone(), key, descriptor)?;

    let error_channel = execute::get_property(channel, "error");
    if error.is_some() && channel_has_subscribers(state, &error_channel) {
        publish(state, Some(&error_channel), std::slice::from_ref(context))?;
    }
    let async_start = execute::get_property(channel, "asyncStart");
    let async_end = execute::get_property(channel, "asyncEnd");
    let stores = channel_stores(state, &async_start);
    let previous = enter_stores(&stores, context);
    if channel_has_subscribers(state, &async_start) {
        publish(state, Some(&async_start), std::slice::from_ref(context))?;
    }
    if channel_has_subscribers(state, &async_end) {
        publish(state, Some(&async_end), std::slice::from_ref(context))?;
    }
    restore_stores(Some(&previous));
    let end = execute::get_property(channel, "end");
    if channel_has_subscribers(state, &end) {
        publish(state, Some(&end), std::slice::from_ref(context))?;
    }
    Ok(())
}

fn channel_object(id: u64, name: Value) -> Value {
    let mut properties = vec![
        (ID.into(), Value::Number(id as f64)),
        ("_index".into(), Value::Number(id as f64)),
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
        (
            "withStoreScope".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_SCOPE),
        ),
        (
            "runStores".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_CHANNEL_RUN_STORES),
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
    if let Some(value) = receiver.filter(|value| {
        channel_data(state, &execute::get_property(value, "start")).is_ok()
            && channel_data(state, &execute::get_property(value, "end")).is_ok()
    }) {
        let names: &[&str] =
            if matches!(execute::get_property(value, BOUNDED), Value::Boolean(true)) {
                &["start", "end"]
            } else {
                &TRACE_CHANNELS
            };
        let active = names
            .iter()
            .any(|name| channel_has_subscribers(state, &execute::get_property(value, name)));
        return Ok(Value::Boolean(active));
    }
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
                .map(|data| {
                    let data = data.borrow();
                    !data.subscribers.is_empty() || !data.stores.is_empty()
                })
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
    let stores = data.borrow().stores.clone();
    let store_active = state.borrow().async_hooks.has_local_store();
    let previous = (!store_active).then(|| enter_stores(&stores, &message));
    for callback in callbacks {
        let result = execute::call(
            &callback,
            &Value::Undefined,
            &[message.clone(), name.clone()],
        );
        if let Err(error) = result {
            if !store_active {
                restore_stores(previous.as_deref());
            }
            let quench_runtime::execute::VmError::Thrown(thrown) = error else {
                return Err(error);
            };
            if state
                .borrow()
                .process
                .uncaught_exception_handlers
                .is_empty()
            {
                return Err(quench_runtime::execute::VmError::Thrown(thrown));
            }
            crate::modules::process::emit(
                state,
                &[
                    Value::String("uncaughtException".into()),
                    thrown,
                    Value::String("uncaughtException".into()),
                ],
            )?;
        }
    }
    if !store_active {
        restore_stores(previous.as_deref());
    }
    Ok(Value::Undefined)
}

/// Run a node:test body inside the stores bound to the test start channel and
/// emit the corresponding start/end/error trace events. Keeping the scope
/// around the whole body (including promise pumping) makes AsyncLocalStorage
/// propagation an ordinary channel fact rather than a runner special case.
pub(crate) fn test_scope(
    state: &Rc<RefCell<HostState>>,
    context: &Value,
    body: impl FnOnce() -> Result<Value, VmError>,
) -> Result<Value, VmError> {
    let name = execute::get_property(context, "name");
    let event = host_api::object(vec![
        ("name".into(), name),
        ("type".into(), Value::String("test".into())),
    ]);
    let start = channel(
        state,
        None,
        &[Value::String("tracing:node.test:start".into())],
    )?;
    let end = channel(
        state,
        None,
        &[Value::String("tracing:node.test:end".into())],
    )?;
    let error = channel(
        state,
        None,
        &[Value::String("tracing:node.test:error".into())],
    )?;
    let emit_root = TEST_ROOT_TRACE_EMITTED.with(|emitted| {
        let was_emitted = *emitted.borrow();
        if !was_emitted {
            *emitted.borrow_mut() = true;
        }
        !was_emitted
    });
    if emit_root {
        let root_event = host_api::object(vec![
            ("name".into(), Value::String("<root>".into())),
            ("type".into(), Value::String("suite".into())),
        ]);
        let root_stores = channel_stores(state, &start);
        let (root_previous, root_errors) = enter_stores_with_errors(&root_stores, &root_event);
        for transform_error in root_errors {
            schedule_uncaught(state, transform_error)?;
        }
        if channel_has_subscribers(state, &start) {
            publish(state, Some(&start), std::slice::from_ref(&root_event))?;
        }
        if channel_has_subscribers(state, &end) {
            publish(state, Some(&end), std::slice::from_ref(&root_event))?;
        }
        restore_stores(Some(&root_previous));
    }
    let stores = channel_stores(state, &start);
    let (previous, transform_errors) = enter_stores_with_errors(&stores, &event);
    for transform_error in transform_errors {
        schedule_uncaught(state, transform_error)?;
    }
    if channel_has_subscribers(state, &start) {
        publish(state, Some(&start), std::slice::from_ref(&event))?;
    }
    let result = body();
    match &result {
        Ok(_) => {
            if channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&event))?;
            }
        }
        Err(VmError::Thrown(thrown)) => {
            let _ = execute::set_property_in_place(&event, "error", thrown.clone());
            if channel_has_subscribers(state, &error) {
                publish(state, Some(&error), std::slice::from_ref(&event))?;
            }
            if channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&event))?;
            }
        }
        Err(_) => {
            if channel_has_subscribers(state, &error) {
                publish(state, Some(&error), std::slice::from_ref(&event))?;
            }
            if channel_has_subscribers(state, &end) {
                publish(state, Some(&end), std::slice::from_ref(&event))?;
            }
        }
    }
    restore_stores(Some(&previous));
    result
}

pub fn bind_store(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let data = channel_data(state, receiver.ok_or_else(|| type_error("channel"))?)?;
    let store = args.first().cloned().unwrap_or(Value::Undefined);
    let transform = match args.get(1).cloned() {
        Some(Value::Undefined) | None => eval_function("(value) => value")?,
        Some(value) => value,
    };
    if !quench_runtime::is_callable(&transform) {
        return Err(type_error("transform"));
    }
    let mut data = data.borrow_mut();
    if let Some(entry) = data
        .stores
        .iter_mut()
        .find(|(current, _)| *current == store)
    {
        entry.1 = transform;
    } else {
        data.stores.push((store, transform));
    }
    Ok(receiver.unwrap().clone())
}

pub fn with_store_scope(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let context = args.first().cloned().unwrap_or(Value::Undefined);
    let stores = channel_stores(state, receiver);
    if stores.is_empty() {
        return Ok(host_api::object(vec![
            (SCOPE_PUBLISHED.into(), Value::Boolean(false)),
            (
                "dispose".into(),
                crate::host::capability(SPEC_DIAGNOSTICS_SCOPE_DISPOSE),
            ),
            (
                "Symbol.dispose".into(),
                crate::host::capability(SPEC_DIAGNOSTICS_SCOPE_DISPOSE),
            ),
        ]));
    }
    let mut previous_values = Vec::with_capacity(stores.len());
    let mut store_values = Vec::with_capacity(stores.len());
    for (store, transform) in &stores {
        let previous = execute::call(&execute::get_property(store, "getStore"), store, &[])
            .ok()
            .unwrap_or(Value::Undefined);
        let transformed =
            execute::call(transform, &Value::Undefined, std::slice::from_ref(&context))?;
        let _ = execute::call(
            &execute::get_property(store, "enterWith"),
            store,
            std::slice::from_ref(&transformed),
        );
        store_values.push(store.clone());
        previous_values.push(previous);
    }
    let scope = host_api::object(vec![
        (SCOPE_PUBLISHED.into(), Value::Boolean(true)),
        (SCOPE_STORE.into(), host_api::array(store_values)),
        (SCOPE_PREVIOUS.into(), host_api::array(previous_values)),
        (SCOPE_ACTIVE.into(), Value::Boolean(true)),
        (
            "dispose".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_SCOPE_DISPOSE),
        ),
        (
            "Symbol.dispose".into(),
            crate::host::capability(SPEC_DIAGNOSTICS_SCOPE_DISPOSE),
        ),
    ]);
    publish(state, Some(receiver), std::slice::from_ref(&context))?;
    Ok(scope)
}

pub fn run_stores(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let message = args.first().cloned().unwrap_or(Value::Undefined);
    let callback = args.get(1).ok_or_else(|| type_error("fn"))?;
    if !quench_runtime::is_callable(callback) {
        return Err(type_error("fn"));
    }
    let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
    let call_args = args.get(3..).unwrap_or(&[]);
    let stores = channel_stores(state, receiver);
    let (previous, transform_errors) = enter_stores_with_errors(&stores, &message);
    if let Err(error) = publish(state, Some(receiver), std::slice::from_ref(&message)) {
        restore_stores(Some(&previous));
        return Err(error);
    }
    let result = execute::call(callback, &this_arg, call_args);
    restore_stores(Some(&previous));
    for error in transform_errors {
        schedule_uncaught(state, error)?;
    }
    result
}

pub fn dispose_store_scope(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("scope"))?;
    if !execute::is_truthy(&execute::get_property(receiver, SCOPE_ACTIVE)) {
        return Ok(Value::Undefined);
    }
    let stores = execute::get_property(receiver, SCOPE_STORE);
    let previous = execute::get_property(receiver, SCOPE_PREVIOUS);
    let end = execute::get_property(receiver, SCOPE_END);
    let end_result = if !matches!(end, Value::Undefined) && channel_has_subscribers(state, &end) {
        let context = execute::get_property(receiver, SCOPE_CONTEXT);
        publish(state, Some(&end), std::slice::from_ref(&context))
    } else {
        Ok(Value::Undefined)
    };
    let length = execute::own_enumerable_keys(&stores).len();
    for index in 0..length {
        let store = execute::get_property(&stores, &index.to_string());
        let value = execute::get_property(&previous, &index.to_string());
        let _ = execute::call(
            &execute::get_property(&store, "enterWith"),
            &store,
            std::slice::from_ref(&value),
        );
    }
    let _ = execute::set_property_in_place(receiver, SCOPE_ACTIVE, Value::Boolean(false));
    end_result.map(|_| Value::Undefined)
}

pub fn unbind_store(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("channel"))?;
    let data = channel_data(state, receiver)?;
    let Some(target) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    let stores = &mut data.borrow_mut().stores;
    let before = stores.len();
    stores.retain(|(store, _)| store != target);
    Ok(Value::Boolean(stores.len() != before))
}

fn enter_stores(stores: &[(Value, Value)], message: &Value) -> Vec<(Value, Value)> {
    enter_stores_with_errors(stores, message).0
}

fn enter_stores_with_errors(
    stores: &[(Value, Value)],
    message: &Value,
) -> (Vec<(Value, Value)>, Vec<Value>) {
    let mut previous_values = Vec::with_capacity(stores.len());
    let mut errors = Vec::new();
    for (store, transform) in stores {
        let Some(previous) =
            execute::call(&execute::get_property(store, "getStore"), store, &[]).ok()
        else {
            continue;
        };
        let value = match execute::call(transform, &Value::Undefined, std::slice::from_ref(message))
        {
            Ok(value) => value,
            Err(VmError::Thrown(error)) => {
                errors.push(error);
                continue;
            }
            Err(_) => continue,
        };
        let _ = execute::call(
            &execute::get_property(store, "enterWith"),
            store,
            std::slice::from_ref(&value),
        );
        previous_values.push((store.clone(), previous));
    }
    (previous_values, errors)
}

fn schedule_uncaught(state: &Rc<RefCell<HostState>>, error: Value) -> Result<(), VmError> {
    let callback = eval_function("function(error) { throw error; }")?;
    crate::modules::process::next_tick(state, &[callback, error]).map(|_| ())
}

fn channel_stores(state: &Rc<RefCell<HostState>>, channel: &Value) -> Vec<(Value, Value)> {
    channel_data(state, channel)
        .ok()
        .map(|data| data.borrow().stores.clone())
        .unwrap_or_default()
}

fn channel_store(state: &Rc<RefCell<HostState>>, channel: &Value) -> Option<(Value, Value)> {
    channel_stores(state, channel).into_iter().next()
}

fn enter_store(store: Option<&(Value, Value)>, message: &Value) -> Option<Value> {
    store.and_then(|(store, transform)| {
        enter_stores(&[(store.clone(), transform.clone())], message)
            .into_iter()
            .next()
            .map(|(_, previous)| previous)
    })
}

fn restore_stores(previous: Option<&[(Value, Value)]>) {
    if let Some(previous) = previous {
        for (store, previous) in previous {
            let _ = execute::call(
                &execute::get_property(store, "enterWith"),
                store,
                std::slice::from_ref(&previous),
            );
        }
    }
}

fn restore_store(store: Option<&(Value, Value)>, previous: Option<Value>) {
    if let (Some((store, _)), Some(previous)) = (store, previous) {
        let _ = execute::call(
            &execute::get_property(store, "enterWith"),
            store,
            std::slice::from_ref(&previous),
        );
    }
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
        .map(|data| {
            let data = data.borrow();
            !data.subscribers.is_empty() || !data.stores.is_empty()
        })
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

fn tracing_type_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(
                "The \"nameOrChannels\" argument must be of type string or an instance of TracingChannel or Object"
                    .into(),
            ),
        ),
    ]))
}

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(&format!("({source})"))
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}
