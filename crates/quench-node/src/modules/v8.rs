//! Rust-owned `v8` compatibility namespace.
//!
//! The embedded engine is not V8, so this module exposes Node's observable
//! shape from one data-driven Rust table.  Serdes uses a small tagged JSON
//! format: it is deliberately private to Quench and only needs to round-trip
//! the JavaScript values represented by this host.

use std::cell::Cell;
use std::rc::Rc;

use crate::host::HostState;
use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::HostCapabilityKind;
use quench_runtime::value::{ArrayBufferData, Uint8ArrayData, Value};

const TAG: &str = "__quench_type";
const SERIALIZER: u16 = 2420;
const DESERIALIZER: u16 = 2421;
const SERIALIZER_WRITE: u16 = 2422;
const SERIALIZER_RELEASE: u16 = 2423;
const DESERIALIZER_READ: u16 = 2424;
const SERIALIZE: u16 = 2425;
const DESERIALIZE: u16 = 2426;
const CACHED_TAG: u16 = 2427;
const SET_FLAGS: u16 = 2428;
const HEAP_STATS: u16 = 2429;
const HEAP_SPACES: u16 = 2430;
const HEAP_CODE: u16 = 2431;
const HEAP_SNAPSHOT: u16 = 2432;
const SNAPSHOT_ON: u16 = 2433;
const SNAPSHOT_READ: u16 = 2434;
const VERSION: u16 = 2435;
const PROMISE_HOOK: u16 = 2436;
const HOOK_ENABLE: u16 = 2437;
const HOOK_DISABLE: u16 = 2438;
const SERIALIZER_HEADER: u16 = 2440;
const DESERIALIZER_HEADER: u16 = 2441;
const SERIALIZER_UINT32: u16 = 2442;
const SERIALIZER_RAW: u16 = 2443;
const SERIALIZER_UINT64: u16 = 2444;
const SERIALIZER_DOUBLE: u16 = 2445;
const DESERIALIZER_UINT32: u16 = 2446;
const DESERIALIZER_RAW: u16 = 2447;
const DESERIALIZER_UINT64: u16 = 2448;
const DESERIALIZER_DOUBLE: u16 = 2449;

thread_local! { static VERSION_TAG: Cell<f64> = const { Cell::new(1.0) }; }

fn cap(kind: u16) -> Value {
    host_api::capability_function_with_properties(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::vm::current_context().realm(),
            kind: HostCapabilityKind::Custom(kind),
        },
        Vec::new(),
    )
}

pub fn build() -> Value {
    let mut pairs = vec![
        ("Serializer", cap(SERIALIZER)),
        ("DefaultSerializer", cap(SERIALIZER)),
        ("Deserializer", cap(DESERIALIZER)),
        ("DefaultDeserializer", cap(DESERIALIZER)),
        ("serialize", cap(SERIALIZE)),
        ("deserialize", cap(DESERIALIZE)),
        ("cachedDataVersionTag", cap(CACHED_TAG)),
        ("getHeapStatistics", cap(HEAP_STATS)),
        ("getHeapSpaceStatistics", cap(HEAP_SPACES)),
        ("getHeapCodeStatistics", cap(HEAP_CODE)),
        ("getHeapSnapshot", cap(HEAP_SNAPSHOT)),
        ("getVersion", cap(VERSION)),
        ("setFlagsFromString", cap(SET_FLAGS)),
    ];
    let hooks = host_api::object(vec![("createHook".into(), cap(PROMISE_HOOK))]);
    pairs.push(("promiseHooks", hooks));
    host_api::object(
        pairs
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
    )
}

pub fn is(kind: u16) -> bool {
    kind >= SERIALIZER && kind <= DESERIALIZER_DOUBLE
}

macro_rules! handlers {
    ($(($name:ident, $id:ident)),+ $(,)?) => { $(
        pub fn $name(_state: &Rc<std::cell::RefCell<HostState>>, receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
            call($id, receiver, args)
        }
    )+ };
}
handlers! {
    (serializer_write_handler, SERIALIZER_WRITE), (serializer_release_handler, SERIALIZER_RELEASE),
    (deserializer_read_handler, DESERIALIZER_READ), (serialize_handler, SERIALIZE),
    (deserialize_handler, DESERIALIZE), (cached_tag_handler, CACHED_TAG), (set_flags_handler, SET_FLAGS),
    (heap_stats_handler, HEAP_STATS), (heap_spaces_handler, HEAP_SPACES), (heap_code_handler, HEAP_CODE),
    (heap_snapshot_handler, HEAP_SNAPSHOT), (snapshot_on_handler, SNAPSHOT_ON), (snapshot_read_handler, SNAPSHOT_READ),
    (version_handler, VERSION), (hook_enable_handler, HOOK_ENABLE),
    (hook_disable_handler, HOOK_DISABLE), (serializer_header_handler, SERIALIZER_HEADER),
    (deserializer_header_handler, DESERIALIZER_HEADER), (serializer_uint32_handler, SERIALIZER_UINT32),
    (serializer_raw_handler, SERIALIZER_RAW), (serializer_uint64_handler, SERIALIZER_UINT64),
    (serializer_double_handler, SERIALIZER_DOUBLE), (deserializer_uint32_handler, DESERIALIZER_UINT32),
    (deserializer_raw_handler, DESERIALIZER_RAW), (deserializer_uint64_handler, DESERIALIZER_UINT64),
    (deserializer_double_handler, DESERIALIZER_DOUBLE),
}

pub fn serializer_construct_handler(
    state: &Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    construct(SERIALIZER, args)
}

pub fn promise_hook_handler(
    state: &Rc<std::cell::RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = promise_hook(args)?;
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let mapped = host_api::object(
        ["init", "before", "after"]
            .into_iter()
            .filter_map(|name| get(&options, name).map(|value| (name.to_string(), value)))
            .chain(get(&options, "settled").map(|value| ("promiseResolve".into(), value)))
            .collect(),
    );
    let hook = crate::modules::async_hooks::create_hook(state, None, &[mapped])?;
    let enable = get(&hook, "enable").unwrap_or(Value::Undefined);
    let _ = quench_runtime::execute::call(&enable, &hook, &[])?;
    Ok(hook)
}
pub fn deserializer_construct_handler(
    state: &Rc<std::cell::RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    construct(DESERIALIZER, args)
}

pub fn call(kind: u16, receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    match kind {
        SERIALIZE => Ok(crate::modules::buffer_proto::make_buffer(
            &serde_json::to_vec(&encode(args.first().unwrap_or(&Value::Undefined)))
                .unwrap_or_default(),
        )),
        DESERIALIZE => decode_bytes(args.first().unwrap_or(&Value::Undefined)),
        CACHED_TAG => Ok(Value::Number(VERSION_TAG.with(Cell::get))),
        SET_FLAGS => set_flags(args),
        HEAP_STATS => Ok(heap_statistics()),
        HEAP_SPACES => Ok(heap_spaces()),
        HEAP_CODE => Ok(host_api::object(vec![
            ("code_and_metadata_size".into(), Value::Number(0.0)),
            ("bytecode_and_metadata_size".into(), Value::Number(0.0)),
            ("external_script_source_size".into(), Value::Number(0.0)),
            ("cpu_profiler_metadata_size".into(), Value::Number(0.0)),
        ])),
        HEAP_SNAPSHOT => Ok(snapshot()),
        SNAPSHOT_ON => snapshot_on(receiver, args),
        SNAPSHOT_READ => Ok(receiver
            .and_then(|value| get(value, "\0snapshot"))
            .unwrap_or_else(|| {
                crate::modules::buffer_proto::make_buffer(b"{\"snapshot\":{\"meta\":{}}}\n")
            })),
        VERSION => Ok(Value::String("v8-embedded".into())),
        PROMISE_HOOK => promise_hook(args),
        HOOK_ENABLE | HOOK_DISABLE => Ok(Value::Undefined),
        SERIALIZER_HEADER | DESERIALIZER_HEADER | SERIALIZER_UINT32 | SERIALIZER_RAW
        | SERIALIZER_UINT64 | SERIALIZER_DOUBLE | DESERIALIZER_UINT32 | DESERIALIZER_RAW
        | DESERIALIZER_UINT64 | DESERIALIZER_DOUBLE => Ok(Value::Undefined),
        SERIALIZER_WRITE => serializer_write(receiver, args),
        SERIALIZER_RELEASE => Ok(receiver
            .and_then(|value| get(value, "\0v8-buffer"))
            .unwrap_or_else(|| crate::modules::buffer_proto::make_buffer(&[]))),
        DESERIALIZER_READ => receiver.map_or_else(
            || Err(VmError::NotCallable),
            |value| decode_bytes(&get(value, "\0v8-buffer").unwrap_or(Value::Undefined)),
        ),
        _ => Err(VmError::NotCallable),
    }
}

fn promise_hook(args: &[Value]) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    for name in ["init", "before", "after", "settled"] {
        let value = get(&options, name).unwrap_or(Value::Undefined);
        let valid = match &value {
            Value::Function(function) => !function.is_async,
            Value::BoundFunction(_) | Value::HostCapability(_) => true,
            _ => false,
        };
        if !matches!(value, Value::Undefined) && !valid {
            return Err(quench_runtime::execute::type_error(&format!(
                "The \"{name}Hook\" argument must be of type function"
            )));
        }
    }
    Ok(host_api::object(vec![
        ("enable".into(), cap(HOOK_ENABLE)),
        ("disable".into(), cap(HOOK_DISABLE)),
    ]))
}

pub fn construct(kind: u16, args: &[Value]) -> Result<Value, VmError> {
    match kind {
        SERIALIZER => Ok(host_api::object(vec![
            ("writeValue".into(), cap(SERIALIZER_WRITE)),
            ("releaseBuffer".into(), cap(SERIALIZER_RELEASE)),
            ("writeHeader".into(), cap(SERIALIZER_HEADER)),
            ("writeUint32".into(), cap(SERIALIZER_UINT32)),
            ("writeRawBytes".into(), cap(SERIALIZER_RAW)),
            ("writeUint64".into(), cap(SERIALIZER_UINT64)),
            ("writeDouble".into(), cap(SERIALIZER_DOUBLE)),
        ])),
        DESERIALIZER => Ok(host_api::object(vec![
            (
                "\0v8-buffer".into(),
                args.first().cloned().unwrap_or(Value::Undefined),
            ),
            ("readValue".into(), cap(DESERIALIZER_READ)),
            ("readHeader".into(), cap(DESERIALIZER_HEADER)),
            ("readUint32".into(), cap(DESERIALIZER_UINT32)),
            ("readRawBytes".into(), cap(DESERIALIZER_RAW)),
            ("readUint64".into(), cap(DESERIALIZER_UINT64)),
            ("readDouble".into(), cap(DESERIALIZER_DOUBLE)),
        ])),
        _ => Err(VmError::NotCallable),
    }
}

fn serializer_write(receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let bytes = crate::modules::buffer_proto::make_buffer(
        &serde_json::to_vec(&encode(args.first().unwrap_or(&Value::Undefined))).unwrap_or_default(),
    );
    quench_runtime::execute::set_property_in_place(receiver, "\0v8-buffer", bytes);
    Ok(Value::Boolean(true))
}

fn set_flags(args: &[Value]) -> Result<Value, VmError> {
    if !matches!(args.first(), Some(Value::String(_))) {
        let received =
            crate::modules::util::invalid_arg_received(args.first().unwrap_or(&Value::Undefined));
        let error = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::TypeError),
            &Value::Undefined,
            &[Value::String(format!(
                "The \"flags\" argument must be of type string.{received}"
            ))],
        )
        .unwrap_or_else(|_| host_api::object(Vec::new()));
        quench_runtime::execute::set_property_in_place(
            &error,
            "code",
            Value::String("ERR_INVALID_ARG_TYPE".into()),
        );
        return Err(VmError::Thrown(error));
    }
    VERSION_TAG.with(|tag| tag.set(tag.get() + 1.0));
    Ok(Value::Undefined)
}

fn heap_statistics() -> Value {
    let number = Value::Number(0.0);
    host_api::object(vec![
        ("total_heap_size".into(), number.clone()),
        ("total_heap_size_executable".into(), number.clone()),
        ("total_physical_size".into(), number.clone()),
        ("total_allocated_bytes".into(), number.clone()),
        ("total_available_size".into(), number.clone()),
        ("used_heap_size".into(), number.clone()),
        (
            "heap_size_limit".into(),
            Value::Number(4.0 * 1024.0 * 1024.0 * 1024.0),
        ),
        ("malloced_memory".into(), number.clone()),
        ("external_memory".into(), number.clone()),
        ("peak_malloced_memory".into(), number.clone()),
        ("does_zap_garbage".into(), number.clone()),
        ("number_of_native_contexts".into(), Value::Number(1.0)),
        ("number_of_detached_contexts".into(), number.clone()),
        ("total_global_handles_size".into(), number.clone()),
        ("used_global_handles_size".into(), number),
    ])
}

fn heap_spaces() -> Value {
    let names = [
        "code_large_object_space",
        "code_space",
        "large_object_space",
        "new_large_object_space",
        "new_space",
        "old_space",
        "read_only_space",
        "shared_large_object_space",
        "shared_space",
        "shared_trusted_large_object_space",
        "shared_trusted_space",
        "trusted_large_object_space",
        "trusted_space",
    ];
    host_api::array(
        names
            .into_iter()
            .map(|name| {
                host_api::object(vec![
                    ("space_name".into(), Value::String(name.into())),
                    ("space_size".into(), Value::Number(0.0)),
                    ("space_used_size".into(), Value::Number(0.0)),
                    ("space_available_size".into(), Value::Number(0.0)),
                    ("physical_space_size".into(), Value::Number(0.0)),
                ])
            })
            .collect(),
    )
}

fn snapshot() -> Value {
    let data = crate::modules::buffer_proto::make_buffer(b"{\"snapshot\":{\"meta\":{}}}\n");
    host_api::object(vec![
        ("\0snapshot".into(), data),
        ("on".into(), cap(SNAPSHOT_ON)),
        ("read".into(), cap(SNAPSHOT_READ)),
    ])
}

fn snapshot_on(receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let event = args.first().and_then(|value| match value {
        Value::String(name) => Some(name.as_str()),
        _ => None,
    });
    let listener = args.get(1).cloned().unwrap_or(Value::Undefined);
    if let Some(listener) = event.zip(Some(listener)).filter(|(_, listener)| {
        matches!(
            listener,
            Value::Function(_) | Value::BoundFunction(_) | Value::HostCapability(_)
        )
    }) {
        let data = receiver
            .and_then(|value| get(value, "\0snapshot"))
            .unwrap_or_else(|| {
                crate::modules::buffer_proto::make_buffer(b"{\"snapshot\":{\"meta\":{}}}\n")
            });
        quench_runtime::execute::call(&listener.1, &Value::Undefined, &[data])?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn get(value: &Value, key: &str) -> Option<Value> {
    quench_runtime::execute::get_property_result(value, key).ok()
}

fn bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::String(text) => Some(text.as_bytes().to_vec()),
        Value::ArrayBuffer(buffer) => Some(buffer.bytes.borrow().clone()),
        Value::Uint8Array(view) => Some(
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec(),
        ),
        _ => None,
    }
}

fn decode_bytes(value: &Value) -> Result<Value, VmError> {
    let bytes =
        bytes(value).ok_or_else(|| quench_runtime::execute::type_error("Unable to deserialize"))?;
    let json = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(decode(&json, false))
}

fn encode(value: &Value) -> serde_json::Value {
    encode_inner(value, 0)
}

fn encode_inner(value: &Value, depth: usize) -> serde_json::Value {
    if depth > 128 {
        return serde_json::Value::Null;
    }
    let tagged = |kind: &str, fields: Vec<(&str, serde_json::Value)>| {
        let mut map = serde_json::Map::new();
        map.insert(TAG.into(), kind.into());
        for (key, value) in fields {
            map.insert(key.into(), value);
        }
        serde_json::Value::Object(map)
    };
    match value {
        Value::Undefined => tagged("undefined", vec![]),
        Value::BigInt(value) => tagged("bigint", vec![("value", value.clone().into())]),
        Value::Number(value) if !value.is_finite() => serde_json::Value::Null,
        Value::Number(value) => serde_json::json!(value),
        Value::Boolean(value) => serde_json::json!(value),
        Value::String(value) => serde_json::json!(value),
        Value::Null => serde_json::Value::Null,
        Value::Uint8Array(view) => {
            let data = view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length]
                .iter()
                .map(|byte| serde_json::json!(byte))
                .collect();
            tagged(
                if crate::modules::buffer::is_buffer(&[value.clone()]) {
                    "buffer"
                } else {
                    "uint8array"
                },
                vec![("data", serde_json::Value::Array(data))],
            )
        }
        Value::Array(_) => {
            let length = get(value, "length")
                .and_then(|value| match value {
                    Value::Number(number) => Some(number.max(0.0) as usize),
                    _ => None,
                })
                .unwrap_or(0);
            serde_json::Value::Array(
                (0..length)
                    .map(|index| {
                        encode_inner(
                            &quench_runtime::execute::get_property(value, &index.to_string()),
                            depth + 1,
                        )
                    })
                    .collect(),
            )
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            let mut map = serde_json::Map::new();
            for key in quench_runtime::execute::own_enumerable_keys(value) {
                map.insert(
                    key.clone(),
                    encode_inner(
                        &quench_runtime::execute::get_property(value, &key),
                        depth + 1,
                    ),
                );
            }
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}

fn decode(value: &serde_json::Value, array_item: bool) -> Value {
    let Some(object) = value.as_object() else {
        return match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(value) => Value::Boolean(*value),
            serde_json::Value::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(value) => Value::String(value.clone()),
            serde_json::Value::Array(values) => {
                host_api::array(values.iter().map(|item| decode(item, true)).collect())
            }
            _ => Value::Undefined,
        };
    };
    match object.get(TAG).and_then(serde_json::Value::as_str) {
        Some("undefined") => Value::Undefined,
        Some("bigint") => Value::BigInt(
            object
                .get("value")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
        ),
        Some(kind @ ("buffer" | "uint8array")) => {
            let bytes = object
                .get("data")
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| value.as_u64().unwrap_or(0) as u8)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let buffer = Rc::new(ArrayBufferData::new(bytes.len()));
            buffer.bytes.borrow_mut().copy_from_slice(&bytes);
            if kind == "buffer" {
                crate::modules::buffer_proto::make_buffer(&bytes)
            } else {
                Value::Uint8Array(Rc::new(Uint8ArrayData::new(buffer, 0, bytes.len())))
            }
        }
        _ => host_api::object(
            object
                .iter()
                .filter_map(|(key, value)| {
                    let item = decode(value, false);
                    (!matches!(item, Value::Undefined) || array_item).then_some((key.clone(), item))
                })
                .collect(),
        ),
    }
}
