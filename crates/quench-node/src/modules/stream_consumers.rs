//! Rust-owned `stream/consumers`.
//!
//! A consumer is one reduction over chunks.  The source protocol is the only
//! branch: WHATWG readers use `read().then(...)`, while Node streams use the
//! EventEmitter edge.  Everything after that edge is shared chunk data.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef};
use quench_runtime::value::{ArrayBufferData, PromiseData, PromiseState, Value};

use crate::host::HostState;

const MODE_BUFFER: u16 = 0;
const MODE_ARRAY_BUFFER: u16 = 1;
const MODE_TEXT: u16 = 2;
const MODE_JSON: u16 = 3;
const MODE_BYTES: u16 = 4;
const MODE_BLOB: u16 = 5;

fn capability(spec: crate::registry::NodeSpec) -> Value {
    crate::host::capability(spec)
}

pub fn build() -> Value {
    host_api::object(vec![
        (
            "buffer".into(),
            capability(crate::registry::SPEC_STREAM_CONSUMER_BUFFER),
        ),
        (
            "arrayBuffer".into(),
            capability(crate::registry::SPEC_STREAM_CONSUMER_ARRAY_BUFFER),
        ),
        (
            "text".into(),
            capability(crate::registry::SPEC_STREAM_CONSUMER_TEXT),
        ),
        (
            "json".into(),
            capability(crate::registry::SPEC_STREAM_CONSUMER_JSON),
        ),
        (
            "bytes".into(),
            capability(crate::registry::SPEC_STREAM_CONSUMER_BYTES),
        ),
        (
            "blob".into(),
            capability(crate::registry::SPEC_STREAM_CONSUMER_BLOB),
        ),
    ])
}

macro_rules! public_handlers {
    ($(($name:ident, $spec:ident, $mode:expr)),+ $(,)?) => { $(
        pub fn $name(state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
            start(state, args.first().cloned().unwrap_or(Value::Undefined), $mode)
        }
    )+ };
}
public_handlers! {
    (buffer, SPEC_STREAM_CONSUMER_BUFFER, MODE_BUFFER),
    (array_buffer, SPEC_STREAM_CONSUMER_ARRAY_BUFFER, MODE_ARRAY_BUFFER),
    (text, SPEC_STREAM_CONSUMER_TEXT, MODE_TEXT),
    (json, SPEC_STREAM_CONSUMER_JSON, MODE_JSON),
    (bytes, SPEC_STREAM_CONSUMER_BYTES, MODE_BYTES),
    (blob, SPEC_STREAM_CONSUMER_BLOB, MODE_BLOB),
}

pub fn reader_step(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Object(_) | Value::ObjectAlias(_)) = args.first() else {
        return Ok(Value::Undefined);
    };
    let context = args.first().cloned().unwrap_or(Value::Undefined);
    let step = args.get(1).cloned().unwrap_or(Value::Undefined);
    if truthy(&step, "done") {
        return finish(state, &context);
    }
    append_chunk(&context, get(&step, "value").unwrap_or(Value::Undefined));
    read_next(state, &context)
}

pub fn reader_reject(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(Value::Object(_) | Value::ObjectAlias(_)), Some(error)) =
        (args.first(), args.get(1))
    {
        reject(args.first().unwrap(), error.clone());
    }
    Ok(Value::Undefined)
}

pub fn event_data(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(context), Some(chunk)) = (args.first(), args.get(1)) {
        append_chunk(context, chunk.clone());
    }
    Ok(Value::Undefined)
}

pub fn event_end(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    args.first()
        .map_or(Ok(Value::Undefined), |context| finish(state, context))
}

pub fn event_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(context), Some(error)) = (args.first(), args.get(1)) {
        reject(context, error.clone());
    }
    Ok(Value::Undefined)
}

fn start(state: &Rc<RefCell<HostState>>, source: Value, mode: u16) -> Result<Value, VmError> {
    let promise = PromiseData::allocate(PromiseState::Pending);
    let result = Value::Promise(promise);
    let context = host_api::object(vec![
        ("\0consumer-promise".into(), result.clone()),
        ("\0consumer-source".into(), source.clone()),
        ("\0consumer-mode".into(), Value::Number(mode as f64)),
        ("\0consumer-chunks".into(), host_api::array(Vec::new())),
    ]);
    if let Some(get_reader) =
        get(&source, "getReader").filter(|value| quench_runtime::is_callable(value))
    {
        match quench_runtime::execute::call(&get_reader, &source, &[]) {
            Ok(reader) => {
                set(&context, "\0consumer-reader", reader);
                if let Err(error) = read_next(state, &context) {
                    reject(&context, vm_error_value(error));
                }
            }
            Err(_) => reject(&context, invalid_state()),
        }
    } else {
        attach_events(state, &source, &context)?;
    }
    Ok(result)
}

fn read_next(state: &Rc<RefCell<HostState>>, context: &Value) -> Result<Value, VmError> {
    let reader = get(context, "\0consumer-reader").ok_or(VmError::NotCallable)?;
    let read = get(&reader, "read").ok_or(VmError::NotCallable)?;
    let pending = quench_runtime::execute::call(&read, &reader, &[])?;
    let on_value = bound(
        crate::registry::SPEC_STREAM_CONSUMER_READER_STEP,
        context.clone(),
    );
    let on_error = bound(
        crate::registry::SPEC_STREAM_CONSUMER_READER_REJECT,
        context.clone(),
    );
    quench_runtime::promise_then(Some(&pending), &[on_value, on_error]).map(|_| Value::Undefined)
}

fn attach_events(
    state: &Rc<RefCell<HostState>>,
    source: &Value,
    context: &Value,
) -> Result<(), VmError> {
    let on = get(source, "on").filter(|value| quench_runtime::is_callable(value));
    let Some(on) = on else {
        reject(context, invalid_stream());
        return Ok(());
    };
    let data = bound(
        crate::registry::SPEC_STREAM_CONSUMER_EVENT_DATA,
        context.clone(),
    );
    let end = bound(
        crate::registry::SPEC_STREAM_CONSUMER_EVENT_END,
        context.clone(),
    );
    let error = bound(
        crate::registry::SPEC_STREAM_CONSUMER_EVENT_ERROR,
        context.clone(),
    );
    quench_runtime::execute::call(&on, source, &[Value::String("data".into()), data])?;
    let once = get(source, "once")
        .filter(|value| quench_runtime::is_callable(value))
        .unwrap_or(on.clone());
    quench_runtime::execute::call(&once, source, &[Value::String("end".into()), end])?;
    quench_runtime::execute::call(&once, source, &[Value::String("error".into()), error])?;
    let _ = state;
    Ok(())
}

fn finish(_state: &Rc<RefCell<HostState>>, context: &Value) -> Result<Value, VmError> {
    let chunks = get(context, "\0consumer-chunks").unwrap_or_else(|| host_api::array(Vec::new()));
    let mode = get(context, "\0consumer-mode")
        .and_then(number)
        .unwrap_or(MODE_BUFFER as f64) as u16;
    let bytes = match collect_bytes(&chunks, matches!(mode, MODE_TEXT | MODE_JSON)) {
        Ok(bytes) => bytes,
        Err(error) => {
            reject(context, error);
            return Ok(Value::Undefined);
        }
    };
    let value = match mode {
        MODE_TEXT => Value::String(String::from_utf8_lossy(&bytes).into_owned()),
        MODE_JSON => quench_runtime::parse_json(&String::from_utf8_lossy(&bytes))
            .map_err(|error| VmError::EvalError(error.to_string()))?,
        MODE_ARRAY_BUFFER => array_buffer_value(&bytes),
        MODE_BYTES => host_api::bytes(&bytes),
        MODE_BLOB => make_blob(&bytes),
        _ => crate::modules::buffer_proto::make_buffer(&bytes),
    };
    if let Some(Value::Promise(promise)) = get(context, "\0consumer-promise") {
        quench_runtime::resolve_promise(&promise, value.clone());
    }
    Ok(value)
}

fn collect_bytes(chunks: &Value, strict: bool) -> Result<Vec<u8>, Value> {
    let length = get(chunks, "length")
        .and_then(number)
        .unwrap_or(0.0)
        .max(0.0) as usize;
    let mut bytes = Vec::new();
    for index in 0..length {
        let chunk = quench_runtime::execute::get_property(chunks, &index.to_string());
        if let Some(data) = crate::modules::crypto::bytes_from_value(&chunk) {
            bytes.extend(data);
        } else if matches!(chunk, Value::String(_)) || !strict {
            bytes.extend(
                quench_runtime::to_string(&chunk)
                    .unwrap_or_default()
                    .into_bytes(),
            );
        } else {
            return Err(invalid_chunk());
        }
    }
    Ok(bytes)
}

fn append_chunk(context: &Value, chunk: Value) {
    let chunks = get(context, "\0consumer-chunks").unwrap_or_else(|| host_api::array(Vec::new()));
    let length = get(&chunks, "length")
        .and_then(number)
        .unwrap_or(0.0)
        .max(0.0) as usize;
    let chunks = quench_runtime::execute::set_property(chunks, &length.to_string(), chunk);
    let chunks =
        quench_runtime::execute::set_property(chunks, "length", Value::Number((length + 1) as f64));
    set(context, "\0consumer-chunks", chunks);
}

fn make_blob(bytes: &[u8]) -> Value {
    let global = quench_runtime::vm::current_global_object();
    if let Some(blob) = get(&global, "Blob").filter(|value| quench_runtime::is_callable(value)) {
        if let Ok(value) = quench_runtime::execute::construct_value(
            &blob,
            &[host_api::array(vec![
                crate::modules::buffer_proto::make_buffer(bytes),
            ])],
        ) {
            if get(&value, "arrayBuffer").is_some_and(|method| quench_runtime::is_callable(&method))
            {
                return value;
            }
        }
    }
    blob_object(bytes)
}

fn blob_object(bytes: &[u8]) -> Value {
    let data = array_buffer_value(bytes);
    let array_buffer = host_api::bound_capability_with_arguments(
        HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: HostCapabilityKind::Custom(
                crate::registry::SPEC_STREAM_CONSUMER_BLOB_ARRAY_BUFFER.cap,
            ),
        },
        vec![data],
    );
    host_api::object(vec![
        ("size".into(), Value::Number(bytes.len() as f64)),
        ("type".into(), Value::String(String::new())),
        ("arrayBuffer".into(), array_buffer),
    ])
}

pub fn blob_array_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let data = args
        .first()
        .cloned()
        .unwrap_or_else(|| array_buffer_value(&[]));
    Ok(quench_runtime::promise_resolve(&[data]))
}

fn array_buffer_value(bytes: &[u8]) -> Value {
    let buffer = Rc::new(ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(bytes);
    Value::ArrayBuffer(buffer)
}
fn bound(spec: crate::registry::NodeSpec, context: Value) -> Value {
    host_api::bound_capability_with_arguments(
        HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: HostCapabilityKind::Custom(spec.cap),
        },
        vec![context],
    )
}
fn get(value: &Value, key: &str) -> Option<Value> {
    quench_runtime::execute::get_property_result(value, key).ok()
}
fn set(value: &Value, key: &str, item: Value) {
    quench_runtime::execute::set_property_in_place(value, key, item);
}
fn number(value: Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(number),
        _ => None,
    }
}
fn truthy(value: &Value, key: &str) -> bool {
    get(value, key).is_some_and(|value| quench_runtime::execute::is_truthy(&value))
}
fn reject(context: &Value, error: Value) {
    if let Some(Value::Promise(promise)) = get(context, "\0consumer-promise") {
        quench_runtime::reject_promise(&promise, error);
    }
}
fn invalid_stream() -> Value {
    let error = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::TypeError),
        &Value::Undefined,
        &[Value::String(
            "The \"stream\" argument must be an instance of Stream".into(),
        )],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    set(&error, "code", Value::String("ERR_INVALID_ARG_TYPE".into()));
    error
}

fn invalid_chunk() -> Value {
    let error = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::TypeError),
        &Value::Undefined,
        &[Value::String(
            "The \"chunk\" argument must be of type string or an instance of Buffer or Uint8Array"
                .into(),
        )],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    set(&error, "code", Value::String("ERR_INVALID_ARG_TYPE".into()));
    error
}

fn invalid_state() -> Value {
    let error = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::Error),
        &Value::Undefined,
        &[Value::String("Invalid state".into())],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()));
    set(&error, "code", Value::String("ERR_INVALID_STATE".into()));
    error
}

fn vm_error_value(error: VmError) -> Value {
    match error {
        VmError::Thrown(value) => value,
        _ => invalid_state(),
    }
}
