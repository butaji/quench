//! Small native subset of `internal/http2/util` used by Node internals.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

thread_local! {
    static OPTIONS_BUFFER: RefCell<Option<Value>> = const { RefCell::new(None) };
    static HTTP2_BINDING_SESSION_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}
use super::http2_asserts;
use super::http2_facts::{
    CONNECTION_HEADERS, HEADER_CONSTANTS, METHOD_CONSTANTS, NUMERIC_CONSTANTS, OPTION_FIELDS,
    SINGLE_VALUE_HEADERS, STATUS_CONSTANTS,
};

/// Bytes emitted at the start of every clear-text HTTP/2 client connection.
/// Keep the wire framing in one Rust-owned helper so the eventual session
/// state machine and the current endpoint boundary cannot diverge on the
/// connection preface or the mandatory initial SETTINGS frame.
pub(crate) fn client_preface() -> Vec<u8> {
    client_preface_with_settings(None)
}

/// Construct a client preface with the caller's initial SETTINGS payload.
/// Keeping the payload at the transport boundary makes the public
/// `localSettings` object and the wire representation derive from one value.
pub(crate) fn client_preface_with_settings(settings: Option<&Value>) -> Vec<u8> {
    let mut bytes = crate::modules::http2_protocol::CONNECTION_PREFACE.to_vec();
    let payload = settings
        .and_then(|settings| packed_settings(&[settings.clone()]).ok())
        .and_then(|value| typed_array_elements(&value))
        .unwrap_or_default();
    bytes.extend_from_slice(
        &crate::modules::http2_protocol::Frame::new(
            crate::modules::http2_protocol::FrameType::Settings,
            0,
            0,
            payload,
        )
        .encode(),
    );
    bytes
}

/// Add Node's stable settings fields to a wire/options subset.  The object is
/// allocated once and then updated in place so repeated property reads retain
/// identity, as they do for a ClientHttp2Session.
pub(crate) fn settings_object(settings: Option<&Value>) -> Value {
    let result = default_settings();
    let custom = host_api::object(Vec::new());
    if matches!(settings, Some(Value::Object(_) | Value::ObjectAlias(_))) {
        let settings = settings.expect("matched settings object");
        for name in [
            "headerTableSize",
            "enablePush",
            "initialWindowSize",
            "maxFrameSize",
            "maxConcurrentStreams",
            "maxHeaderListSize",
            "maxHeaderSize",
            "enableConnectProtocol",
        ] {
            let value = execute::get_property(settings, name);
            if !matches!(value, Value::Undefined) {
                set_property(&result, name, value);
            }
        }
        let supplied_custom = execute::get_property(settings, "customSettings");
        if matches!(supplied_custom, Value::Object(_) | Value::ObjectAlias(_)) {
            for key in execute::own_enumerable_keys(&supplied_custom) {
                set_property(&custom, &key, execute::get_property(&supplied_custom, &key));
            }
        }
    }
    set_property(&result, "customSettings", custom);
    result
}

/// Decode one received SETTINGS payload into the public settings shape.
pub(crate) fn settings_from_payload(payload: &[u8]) -> Value {
    let decoded = unpacked_settings(&[crate::modules::buffer_proto::make_buffer(payload)])
        .unwrap_or_else(|_| host_api::object(Vec::new()));
    let result = settings_object(Some(&decoded));
    let custom = execute::get_property(&decoded, "customSettings");
    let result_custom = execute::get_property(&result, "customSettings");
    if matches!(custom, Value::Object(_) | Value::ObjectAlias(_)) {
        for key in execute::own_enumerable_keys(&custom) {
            set_property(&result_custom, &key, execute::get_property(&custom, &key));
        }
    }
    result
}

pub(crate) fn filter_custom_settings(settings: &Value, allowed: Option<&Value>) {
    let Some(allowed) = allowed else { return };
    let custom = execute::get_property(settings, "customSettings");
    if !matches!(custom, Value::Object(_) | Value::ObjectAlias(_)) {
        return;
    }
    let allowed = execute::own_enumerable_keys(allowed)
        .into_iter()
        .filter_map(|key| match execute::get_property(allowed, &key) {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .filter(|value| value.is_finite() && value.fract() == 0.0)
        .map(|value| (value as u32).to_string())
        .collect::<HashSet<_>>();
    let filtered = host_api::object(
        execute::own_enumerable_keys(&custom)
            .into_iter()
            .filter(|key| allowed.contains(key))
            .map(|key| (key.clone(), execute::get_property(&custom, &key)))
            .collect(),
    );
    set_property(settings, "customSettings", filtered);
}

pub(crate) fn packed_settings_payload(settings: &Value) -> Option<Vec<u8>> {
    packed_settings(&[settings.clone()])
        .ok()
        .and_then(|value| typed_array_elements(&value))
}

pub(crate) fn configure_session_settings(socket: &Value, local: Option<&Value>) {
    set_property(socket, "localSettings", settings_object(local));
    set_property(socket, "remoteSettings", settings_object(None));
}

fn update_local_settings(socket: &Value, update: &Value) {
    let current = execute::get_property(socket, "localSettings");
    if !matches!(current, Value::Object(_) | Value::ObjectAlias(_)) {
        set_property(socket, "localSettings", settings_object(Some(update)));
        return;
    }
    for name in [
        "headerTableSize",
        "enablePush",
        "initialWindowSize",
        "maxFrameSize",
        "maxConcurrentStreams",
        "maxHeaderListSize",
        "maxHeaderSize",
        "enableConnectProtocol",
    ] {
        if execute::has_own_property(update, name) {
            set_property(&current, name, execute::get_property(update, name));
        }
    }
    let custom = execute::get_property(&current, "customSettings");
    let update_custom = execute::get_property(update, "customSettings");
    if matches!(custom, Value::Object(_) | Value::ObjectAlias(_))
        && matches!(update_custom, Value::Object(_) | Value::ObjectAlias(_))
    {
        for key in execute::own_enumerable_keys(&update_custom) {
            set_property(&custom, &key, execute::get_property(&update_custom, &key));
        }
    }
}

/// Shared private key used by Node's internal HTTP/2 tests to retrieve the
/// transport socket backing a ClientHttp2Session.  Quench represents
/// well-known symbols as private string keys; exporting the same key from the
/// internal util module keeps session and test-side property access identical.
pub(crate) const HTTP2_SOCKET_SYMBOL: &str = "Symbol.nodejs.http2.kSocket\0quench";

fn http2_capability(kind: &str) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
        vec![Value::String(kind.into())],
    )
}

pub(crate) fn coded_error(
    kind: quench_runtime::ops::Builtin,
    code: &str,
    message: String,
) -> VmError {
    let error = quench_runtime::builtins::error(kind, &[Value::String(message)]);
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String(code.into()),
    ))
}

pub(crate) fn quoted(value: &Value) -> String {
    match value {
        Value::String(value) => format!("\"{value}\""),
        _ => crate::modules::util::inspect(value),
    }
}

pub fn module() -> Value {
    let make = |kind: &str| {
        host_api::bound_capability_with_arguments(
            crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
            vec![Value::String(kind.into())],
        )
    };
    let constructor = make("nghttpError");
    let prototype = host_api::object(vec![("toString".into(), make("nghttpToString"))]);
    let _ = execute::set_property_in_place(&prototype, "constructor", constructor.clone());
    let constructor = execute::set_property(constructor, "prototype", prototype.clone());
    let global = quench_runtime::vm::current_global_object();
    execute::set_property_in_place(
        &global,
        "\0quench:http2-nghttp-constructor",
        constructor.clone(),
    );
    execute::set_property_in_place(&global, "\0quench:http2-nghttp-prototype", prototype);
    let module = crate::host::namespace_object_from_pairs(vec![
        ("assertValidPseudoHeader".into(), make("pseudo")),
        (
            "assertValidPseudoHeaderResponse".into(),
            make("pseudoResponse"),
        ),
        (
            "assertValidPseudoHeaderTrailer".into(),
            make("pseudoTrailer"),
        ),
        ("assertIsObject".into(), make("object")),
        ("assertIsArray".into(), make("array")),
        ("assertWithinRange".into(), make("range")),
        ("sessionName".into(), make("sessionName")),
        ("updateOptionsBuffer".into(), make("updateOptionsBuffer")),
        ("getAuthority".into(), make("getAuthority")),
        ("buildNgHeaderString".into(), make("buildNgHeaderString")),
        ("toHeaderObject".into(), make("toHeaderObject")),
        ("NghttpError".into(), constructor),
        ("kSocket".into(), Value::String(HTTP2_SOCKET_SYMBOL.into())),
    ]);
    let global = quench_runtime::vm::current_global_object();
    // `internalBinding('http2')` is allowed to run before the public
    // `require('http2')` path. Reuse the host-installed binding in that case;
    // replacing it would discard prototype mutations made by internal tests
    // before the public module is loaded.
    let binding = match execute::get_property(&global, "__quenchHttp2Binding") {
        value @ (Value::Object(_) | Value::ObjectAlias(_)) => value,
        _ => binding(),
    };
    let descriptor = host_api::object(vec![
        ("value".into(), binding),
        ("writable".into(), Value::Boolean(true)),
        ("configurable".into(), Value::Boolean(true)),
        ("enumerable".into(), Value::Boolean(false)),
    ]);
    let _ = execute::define_property(global, "__quenchHttp2Binding", descriptor);
    module
}

pub fn sensitive_headers() -> Value {
    Value::String("Symbol.nodejs.http2.sensitiveHeaders\0quench".into())
}

/// The defaults used by the Rust HTTP/2 settings codec.  This is deliberately
/// independent of any session implementation: callers can construct and
/// inspect SETTINGS payloads even when no HTTP/2 transport is available.
fn default_settings() -> Value {
    host_api::object(vec![
        ("headerTableSize".into(), Value::Number(4096.0)),
        ("enablePush".into(), Value::Boolean(true)),
        ("initialWindowSize".into(), Value::Number(4_194_304.0)),
        ("maxFrameSize".into(), Value::Number(16_384.0)),
        (
            "maxConcurrentStreams".into(),
            Value::Number(4_294_967_295.0),
        ),
        ("maxHeaderSize".into(), Value::Number(65_535.0)),
        ("maxHeaderListSize".into(), Value::Number(65_535.0)),
        ("enableConnectProtocol".into(), Value::Boolean(false)),
        ("customSettings".into(), host_api::object(Vec::new())),
    ])
}

fn packed_settings(values: &[Value]) -> Result<Value, VmError> {
    let settings = values.first().unwrap_or(&Value::Undefined);
    if matches!(settings, Value::Undefined) {
        return Ok(crate::modules::buffer_proto::make_buffer(&[]));
    }
    if !matches!(settings, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_ARG_TYPE",
            format!(
                "The \"settings\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(settings)
            ),
        ));
    }

    let mut entries = Vec::new();
    push_numeric_setting(
        settings,
        "headerTableSize",
        1,
        0.0,
        u32::MAX as f64,
        &mut entries,
    )?;
    push_boolean_setting(settings, "enablePush", 2, &mut entries)?;
    push_numeric_setting(
        settings,
        "maxConcurrentStreams",
        3,
        0.0,
        u32::MAX as f64,
        &mut entries,
    )?;
    push_numeric_setting(
        settings,
        "initialWindowSize",
        4,
        0.0,
        2_147_483_647.0,
        &mut entries,
    )?;
    push_numeric_setting(
        settings,
        "maxFrameSize",
        5,
        16_384.0,
        16_777_215.0,
        &mut entries,
    )?;
    let header_list_name = if execute::has_own_property(settings, "maxHeaderListSize") {
        "maxHeaderListSize"
    } else {
        "maxHeaderSize"
    };
    push_numeric_setting(
        settings,
        header_list_name,
        6,
        0.0,
        u32::MAX as f64,
        &mut entries,
    )?;
    push_boolean_setting(settings, "enableConnectProtocol", 8, &mut entries)?;

    let custom = execute::get_property(settings, "customSettings");
    if !matches!(custom, Value::Undefined) {
        if !matches!(custom, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"customSettings\" property must be of type object.{}",
                    crate::modules::util::invalid_arg_received(&custom)
                ),
            ));
        }
        let keys = execute::own_enumerable_keys(&custom);
        if keys.len() > 10 {
            return Err(coded_error(
                // Node reports this cardinality violation as a plain Error;
                // value/range violations below retain their RangeError type.
                quench_runtime::ops::Builtin::Error,
                "ERR_HTTP2_TOO_MANY_CUSTOM_SETTINGS",
                "Maximum number of custom settings is 10".into(),
            ));
        }
        for key in keys {
            let Ok(id) = key.parse::<u32>() else {
                return invalid_setting(&key, &execute::get_property(&custom, &key), false);
            };
            if !(9..=u16::MAX as u32).contains(&id) {
                return invalid_setting(&key, &execute::get_property(&custom, &key), false);
            }
            let value = execute::get_property(&custom, &key);
            let number = setting_number(&key, &value, 0.0, u32::MAX as f64)?;
            entries.push((id as u16, number as u32));
        }
    }
    let mut bytes = Vec::with_capacity(entries.len() * 6);
    for (id, value) in entries {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

fn push_numeric_setting(
    settings: &Value,
    name: &str,
    id: u16,
    min: f64,
    max: f64,
    entries: &mut Vec<(u16, u32)>,
) -> Result<(), VmError> {
    if !execute::has_own_property(settings, name) {
        return Ok(());
    }
    let value = execute::get_property(settings, name);
    let number = setting_number(name, &value, min, max)?;
    entries.push((id, number as u32));
    Ok(())
}

fn push_boolean_setting(
    settings: &Value,
    name: &str,
    id: u16,
    entries: &mut Vec<(u16, u32)>,
) -> Result<(), VmError> {
    if !execute::has_own_property(settings, name) {
        return Ok(());
    }
    let value = execute::get_property(settings, name);
    let Value::Boolean(value) = value else {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_HTTP2_INVALID_SETTING_VALUE",
            format!(
                "Invalid value for setting \"{name}\": {}",
                setting_display(&value)
            ),
        ));
    };
    entries.push((id, value as u32));
    Ok(())
}

fn setting_number(name: &str, value: &Value, min: f64, max: f64) -> Result<f64, VmError> {
    let number = match value {
        Value::Number(number) => *number,
        _ => {
            return invalid_setting(name, value, true);
        }
    };
    if !number.is_finite() || number.fract() != 0.0 || number < min || number > max {
        return invalid_setting(name, value, false);
    }
    Ok(number)
}

fn invalid_setting<T>(name: &str, value: &Value, type_error: bool) -> Result<T, VmError> {
    let kind = if type_error {
        quench_runtime::ops::Builtin::TypeError
    } else {
        quench_runtime::ops::Builtin::RangeError
    };
    Err(coded_error(
        kind,
        "ERR_HTTP2_INVALID_SETTING_VALUE",
        format!(
            "Invalid value for setting \"{name}\": {}",
            setting_display(value)
        ),
    ))
}

fn setting_display(value: &Value) -> String {
    quench_runtime::execute::to_js_string(value)
        .unwrap_or_else(|_| crate::modules::util::inspect(value))
}

fn unpacked_settings(values: &[Value]) -> Result<Value, VmError> {
    let packed = values.first().unwrap_or(&Value::Undefined);
    let bytes = typed_array_elements(packed).ok_or_else(|| {
        coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_ARG_TYPE",
            format!(
                "The \"buf\" argument must be an instance of Buffer or TypedArray.{}",
                invalid_buffer_received(packed)
            ),
        )
    })?;
    if bytes.len() % 6 != 0 {
        return Err(coded_error(
            quench_runtime::ops::Builtin::RangeError,
            "ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH",
            "Packed settings length must be a multiple of six".into(),
        ));
    }
    let validate = matches!(
        values.get(1),
        Some(Value::Object(_) | Value::ObjectAlias(_))
    ) && matches!(
        execute::get_property(values.get(1).unwrap(), "validate"),
        Value::Boolean(true)
    );
    let mut result = default_settings();
    let mut custom = host_api::object(Vec::new());
    for chunk in bytes.chunks_exact(6) {
        let id = u16::from_be_bytes([chunk[0], chunk[1]]);
        let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
        match id {
            1 => set_number(&mut result, "headerTableSize", value),
            2 => set_bool(&mut result, "enablePush", value != 0),
            3 => set_number(&mut result, "maxConcurrentStreams", value),
            4 => {
                if validate && value > 2_147_483_647 {
                    return invalid_setting(
                        "initialWindowSize",
                        &Value::Number(value as f64),
                        false,
                    );
                }
                set_number(&mut result, "initialWindowSize", value)
            }
            5 => {
                if validate && !(16_384..=16_777_215).contains(&value) {
                    return invalid_setting("maxFrameSize", &Value::Number(value as f64), false);
                }
                set_number(&mut result, "maxFrameSize", value)
            }
            6 => {
                set_number(&mut result, "maxHeaderListSize", value);
                set_number(&mut result, "maxHeaderSize", value);
            }
            8 => set_bool(&mut result, "enableConnectProtocol", value != 0),
            _ => set_number(&mut custom, &id.to_string(), value),
        }
    }
    set_property(&mut result, "customSettings", custom);
    Ok(result)
}

fn invalid_buffer_received(value: &Value) -> String {
    if matches!(value, Value::DataView(_)) {
        " Received an instance of DataView".into()
    } else {
        crate::modules::util::invalid_arg_received(value)
    }
}

fn set_number(object: &Value, name: &str, value: impl Into<f64>) {
    let _ = execute::set_property_in_place(object, name, Value::Number(value.into()));
}

fn set_bool(object: &Value, name: &str, value: bool) {
    let _ = execute::set_property_in_place(object, name, Value::Boolean(value));
}

fn set_property(object: &Value, name: &str, value: Value) {
    let _ = execute::set_property_in_place(object, name, value);
}

fn typed_array_elements(value: &Value) -> Option<Vec<u8>> {
    let length = match value {
        Value::Uint8Array(view) => view.length,
        Value::Uint8ClampedArray(view) => view.length,
        Value::Int8Array(view) => view.length,
        Value::Uint16Array(view) => view.length,
        Value::Int16Array(view) => view.length,
        Value::Uint32Array(view) => view.length,
        Value::Int32Array(view) => view.length,
        Value::Float32Array(view) => view.length,
        Value::Float64Array(view) => view.length,
        Value::BigInt64Array(view) => view.length,
        Value::BigUint64Array(view) => view.length,
        _ => return None,
    };
    Some(
        (0..length)
            .filter_map(|index| {
                match quench_runtime::to_number(&execute::get_property(value, &index.to_string())) {
                    Ok(number) if number.is_finite() => Some(number as u8),
                    _ => None,
                }
            })
            .collect(),
    )
}

pub fn binding() -> Value {
    let session = http2_binding_session_constructor();
    let error_string = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
        vec![Value::String("errorString".into())],
    );
    host_api::object(vec![
        ("constants".into(), header_constants()),
        ("optionsBuffer".into(), options_buffer()),
        ("Http2Session".into(), session),
        ("nghttp2ErrorString".into(), error_string),
    ])
}

fn http2_binding_session_constructor() -> Value {
    HTTP2_BINDING_SESSION_PROTOTYPE.with(|stored| {
        let prototype = stored
            .borrow_mut()
            .get_or_insert_with(|| {
                host_api::object(vec![("\0quench:host:mutable".into(), Value::Boolean(true))])
            })
            .clone();
        let session =
            host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
        execute::set_property(session, "prototype", prototype)
    })
}

fn header_constants() -> Value {
    constants()
}

/// Build the public `http2.constants` object from the canonical constant
/// tables.  The same object is also returned by the internal binding, which
/// keeps numeric error/frame values and header/method spellings identical for
/// Node's public and internal consumers.
pub(crate) fn constants() -> Value {
    let mut properties = Vec::with_capacity(
        NUMERIC_CONSTANTS.len()
            + STATUS_CONSTANTS.len()
            + METHOD_CONSTANTS.len()
            + HEADER_CONSTANTS.len(),
    );
    properties.extend(
        NUMERIC_CONSTANTS
            .iter()
            .map(|(name, value)| ((*name).into(), Value::Number(*value))),
    );
    properties.extend(
        STATUS_CONSTANTS
            .iter()
            .map(|(name, value)| ((*name).into(), Value::Number(*value))),
    );
    properties.extend(
        METHOD_CONSTANTS
            .iter()
            .map(|(name, value)| ((*name).into(), Value::String((*value).into()))),
    );
    properties.extend(
        HEADER_CONSTANTS
            .iter()
            .map(|(name, value)| ((*name).into(), Value::String((*value).into()))),
    );
    host_api::object(properties)
}

fn options_buffer() -> Value {
    let global = quench_runtime::vm::current_global_object();
    let binding = execute::get_property(&global, "__quenchHttp2Binding");
    let shared = execute::get_property(&binding, "optionsBuffer");
    if matches!(shared, Value::Array(_)) {
        return shared;
    }
    OPTIONS_BUFFER.with(|stored| {
        let mut stored = stored.borrow_mut();
        stored
            .get_or_insert_with(|| host_api::array(vec![Value::Number(0.0); 14]))
            .clone()
    })
}

pub fn dispatch(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Value::String(kind) = args.first().cloned().unwrap_or(Value::Undefined) else {
        return Err(VmError::NotCallable);
    };
    let values = &args[1..];
    match kind.as_str() {
        "nghttpError" => nghttp_error(values),
        "nghttpToString" => nghttp_to_string(_receiver),
        "errorString" => nghttp_error_string(values),
        "defaultSettings" => Ok(default_settings()),
        "packedSettings" => packed_settings(values),
        "unpackedSettings" => unpacked_settings(values),
        "updateOptionsBuffer" => update_options_buffer(values),
        "getAuthority" => get_authority(values),
        "buildNgHeaderString" => build_ng_header_string(values),
        "toHeaderObject" => to_header_object(values),
        "pseudo" => http2_asserts::pseudo(values.first().unwrap_or(&Value::Undefined)),
        "pseudoResponse" => {
            let key = values.first().unwrap_or(&Value::Undefined);
            if !matches!(key, Value::String(value) if value == ":status") {
                Err(coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_HTTP2_INVALID_PSEUDOHEADER",
                    format!(
                        "{} is an invalid pseudoheader or is used incorrectly",
                        quoted(key)
                    ),
                ))
            } else {
                Ok(Value::Undefined)
            }
        }
        "pseudoTrailer" => Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_HTTP2_INVALID_PSEUDOHEADER",
            format!(
                "{} is an invalid pseudoheader or is used incorrectly",
                quoted(values.first().unwrap_or(&Value::Undefined))
            ),
        )),
        "object" => http2_asserts::object(values),
        "array" => http2_asserts::array(values),
        "range" => http2_asserts::range(values),
        "sessionName" => session_name(values),
        "connect" => connect(state, values),
        "sessionRequest" => session_request(state, _receiver, values),
        "sessionConnect" => session_connect(values),
        "sessionClose" => session_close(state, _receiver, values),
        "sessionInvalidMethod" => session_invalid_method(_receiver),
        "sessionMethod" => session_method(state, _receiver, values),
        "streamWrite" => stream_write(state, _receiver, values),
        "streamEnd" => stream_end(state, _receiver, values),
        "streamPriority" => stream_priority(_receiver),
        "streamClose" => stream_close(state, _receiver, values),
        "streamDestroy" => stream_destroy(state, _receiver, values),
        "streamAbort" => stream_abort(state, values),
        "streamRespond" => stream_respond(state, _receiver, values),
        "streamPushStream" => stream_push_stream(state, _receiver, values),
        "streamSetEncoding" => stream_set_encoding(state, _receiver, values),
        "streamResume" | "streamPause" => Ok(_receiver.cloned().unwrap_or(Value::Undefined)),
        "compatResponseWriteHead" => compat_response_write_head(state, _receiver, values),
        "compatResponseWrite" => compat_response_write(state, _receiver, values),
        "compatResponseEnd" => compat_response_end(state, _receiver, values),
        "compatResponseDestroy" => compat_response_destroy(state, _receiver, values),
        "createServer" => create_server(state, values, false),
        "createSecureServer" => create_server(state, values, true),
        _ => Err(VmError::NotCallable),
    }
}

/// Establish the underlying TCP endpoint for an HTTP/2 client session.
///
/// The HTTP/2 protocol/session layer is not implemented yet, but endpoint
/// setup is still a reusable host boundary: it delegates to the canonical
/// `net.connect` implementation (including user-overridden `net.connect`),
/// preserves URL/options normalization, and routes callback errors through
/// the same lifecycle object. Callers that need HTTP/2 framing remain
/// stopped at that explicit capability boundary.
fn connect(state: &Rc<RefCell<HostState>>, values: &[Value]) -> Result<Value, VmError> {
    let authority = values.first().unwrap_or(&Value::Undefined);
    let extra_options = values
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let callback = values
        .iter()
        .skip(1)
        .find(|value| quench_runtime::is_callable(value))
        .cloned();
    let target = connect_target_options(authority, extra_options)?;
    let secure = execute::to_js_string(&execute::get_property(&target, "protocol"))
        .unwrap_or_default()
        .eq_ignore_ascii_case("https:");
    if secure
        && matches!(
            execute::get_property(&target, "ALPNProtocols"),
            Value::Undefined
        )
    {
        // RFC 7301 negotiation is part of the HTTPS HTTP/2 endpoint fact;
        // keep it on the normalized options object shared with tls.connect.
        execute::set_property_in_place(
            &target,
            "ALPNProtocols",
            host_api::array(vec![Value::String("h2".into())]),
        );
    }
    // The HTTP/2 session owns the handshake lifecycle. Mark this normalized
    // option set so the TLS host can defer certificate rejection while a
    // session AbortSignal is still able to cancel the opening transport.
    if secure {
        execute::set_property_in_place(&target, "\0quench:http2-session", Value::Boolean(true));
    }
    let create_connection = execute::get_property(&target, "createConnection");
    if quench_runtime::is_callable(&create_connection) {
        let socket = execute::call(
            &create_connection,
            &Value::Undefined,
            &[authority.clone(), target.clone()],
        )?;
        remember_http2_authority(&socket, &target);
        execute::set_property_in_place(
            &socket,
            crate::modules::http2_protocol::CLIENT_MARKER,
            Value::Boolean(true),
        );
        crate::modules::net::register_http2_session(
            state,
            &socket,
            crate::modules::http2_protocol::Role::Client,
        );
        let write = execute::get_property(&socket, "write");
        if quench_runtime::is_callable(&write) {
            let target_settings = execute::get_property(&target, "settings");
            let settings = matches!(target_settings, Value::Object(_) | Value::ObjectAlias(_))
                .then_some(&target_settings);
            let preface =
                crate::modules::buffer_proto::make_buffer(&client_preface_with_settings(settings));
            execute::call(&write, &socket, &[preface])?;
        }
        if matches!(execute::get_property(&socket, "close"), Value::Undefined) {
            let destroy = execute::get_property(&socket, "destroy");
            if quench_runtime::is_callable(&destroy) {
                execute::set_property_in_place(&socket, "close", destroy);
            }
        }
        decorate_client_session(&socket, secure)?;
        let target_settings = execute::get_property(&target, "settings");
        configure_session_settings(
            &socket,
            matches!(target_settings, Value::Object(_) | Value::ObjectAlias(_))
                .then_some(&target_settings),
        );
        set_ping_limit(&socket, &target);
        if let Some(callback) = callback {
            // `createConnection` may return either an already-connected
            // socket or one that is still opening.  Preserve the transport's
            // connect fact and invoke the HTTP/2 session callback exactly
            // once in either case.
            let connected = transport_connected(state, &socket);
            if connected {
                invoke_session_callback(&callback, &socket)?;
            } else {
                let listener = session_callback(&callback, &socket);
                let once = execute::get_property(&socket, "once");
                if quench_runtime::is_callable(&once) {
                    execute::call(&once, &socket, &[Value::String("connect".into()), listener])?;
                    // `http2.connect` reports opening failures through its
                    // callback as the sole error argument.  Registering the
                    // same callback on the transport's one-shot error event
                    // lets util.promisify reject without treating a socket
                    // error as a successful session value.
                    execute::call(
                        &once,
                        &socket,
                        &[Value::String("error".into()), callback.clone()],
                    )?;
                }
            }
        }
        return Ok(socket);
    }
    let module_name = if secure { "tls" } else { "net" };
    let net = crate::modules::require::require(state, &[Value::String(module_name.into())])?;
    let net_connect = execute::get_property(&net, "connect");
    if !quench_runtime::is_callable(&net_connect) {
        return Err(VmError::NotCallable);
    }
    let net_args = vec![target.clone()];
    let socket = execute::call(&net_connect, &Value::Undefined, &net_args)?;
    remember_http2_authority(&socket, &target);
    execute::set_property_in_place(
        &socket,
        crate::modules::http2_protocol::CLIENT_MARKER,
        Value::Boolean(true),
    );
    crate::modules::net::register_http2_session(
        state,
        &socket,
        crate::modules::http2_protocol::Role::Client,
    );
    // `net.connect` buffers writes until its non-blocking endpoint is
    // connected, so the protocol preface follows the same transport path as
    // application writes and is not lost before the first pump tick.
    let write = execute::get_property(&socket, "write");
    let target_settings = execute::get_property(&target, "settings");
    if quench_runtime::is_callable(&write) {
        let settings = matches!(target_settings, Value::Object(_) | Value::ObjectAlias(_))
            .then_some(&target_settings);
        execute::call(
            &write,
            &socket,
            &[crate::modules::buffer_proto::make_buffer(
                &client_preface_with_settings(settings),
            )],
        )?;
    }
    // A raw net.Socket has `destroy()` rather than the client-session
    // `close()` spelling. Keep endpoint teardown available to callers that
    // only need connection lifecycle management; request/session methods
    // still require the unimplemented HTTP/2 protocol layer.
    if matches!(execute::get_property(&socket, "close"), Value::Undefined) {
        let destroy = execute::get_property(&socket, "destroy");
        if quench_runtime::is_callable(&destroy) {
            execute::set_property_in_place(&socket, "close", destroy);
        }
    }
    decorate_client_session(&socket, secure)?;
    configure_session_settings(
        &socket,
        matches!(target_settings, Value::Object(_) | Value::ObjectAlias(_))
            .then_some(&target_settings),
    );
    set_ping_limit(&socket, &target);
    if let Some(callback) = callback {
        let connected = transport_connected(state, &socket);
        if connected {
            invoke_session_callback(&callback, &socket)?;
        } else {
            let listener = session_callback(&callback, &socket);
            let once = execute::get_property(&socket, "once");
            if quench_runtime::is_callable(&once) {
                execute::call(&once, &socket, &[Value::String("connect".into()), listener])?;
                execute::call(
                    &once,
                    &socket,
                    &[Value::String("error".into()), callback.clone()],
                )?;
            }
        }
    }
    Ok(socket)
}

fn set_ping_limit(socket: &Value, options: &Value) {
    if let Value::Number(limit) = execute::get_property(options, "maxOutstandingPings") {
        if limit.is_finite() && limit >= 0.0 && limit.fract() == 0.0 {
            execute::set_property_in_place(
                socket,
                HTTP2_MAX_OUTSTANDING_PINGS,
                Value::Number(limit),
            );
        }
    }
}

fn remember_http2_authority(socket: &Value, target: &Value) {
    let host = execute::to_js_string(&execute::get_property(target, "host")).ok();
    let port = execute::to_js_string(&execute::get_property(target, "port")).ok();
    let Some(host) = host.filter(|value| !value.is_empty()) else {
        return;
    };
    let authority = port
        .filter(|value| !value.is_empty())
        .map_or(host.clone(), |port| format!("{host}:{port}"));
    execute::set_property_in_place(
        socket,
        "\0quench:http2-authority",
        Value::String(authority.into()),
    );
}

fn session_callback(callback: &Value, socket: &Value) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
        vec![
            Value::String("sessionConnect".into()),
            callback.clone(),
            socket.clone(),
        ],
    )
}

/// A custom `createConnection` may return a socket whose host entry has
/// already announced `connect` (the common case for an already-open net or
/// TLS socket).  Keep the host record as the authoritative fact when present,
/// while using the public open-state tuple for foreign duplex implementations
/// that are not registered in `NetState`.
fn transport_connected(state: &Rc<RefCell<HostState>>, socket: &Value) -> bool {
    let registered = crate::modules::net::net_id(socket)
        .and_then(|id| state.borrow().net.sockets.get(&id).cloned())
        .is_some_and(|entry| entry.borrow().connect_announced);
    registered
        || (matches!(
            execute::get_property(socket, "connecting"),
            Value::Boolean(false)
        ) && matches!(
            execute::get_property(socket, "readyState"),
            Value::String(state) if state == "open"
        ) && !matches!(
            execute::get_property(socket, "destroyed"),
            Value::Boolean(true)
        ))
}

fn invoke_session_callback(callback: &Value, socket: &Value) -> Result<(), VmError> {
    execute::call(callback, &Value::Undefined, &[socket.clone()]).map(|_| ())
}

fn session_connect(values: &[Value]) -> Result<Value, VmError> {
    let callback = values.first().ok_or(VmError::NotCallable)?;
    let socket = values.get(1).ok_or(VmError::NotCallable)?;
    invoke_session_callback(callback, socket)?;
    Ok(Value::Undefined)
}

fn session_capability(kind: &str) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
        vec![Value::String(kind.into())],
    )
}

const HTTP2_MAX_OUTSTANDING_PINGS: &str = "\0quench:http2:max-outstanding-pings";

fn ping_error() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("HTTP2 ping cancelled".into())],
    );
    execute::set_property(error, "code", Value::String("ERR_HTTP2_PING_CANCEL".into()))
}

fn ping_payload(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Uint8Array(_)
        | Value::Int8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Int16Array(_)
        | Value::Uint16Array(_)
        | Value::Int32Array(_)
        | Value::Uint32Array(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::DataView(_) => crate::modules::crypto::bytes_from_value(value),
        _ => None,
    }
}

fn invoke_ping_callback(
    state: &Rc<RefCell<HostState>>,
    pending: crate::modules::net::PendingHttp2Ping,
    error: Option<Value>,
) -> Result<(), VmError> {
    let callback = pending.callback;
    let resource = pending.resource;
    if quench_runtime::is_callable(&callback) {
        crate::modules::async_hooks::resource_before(state, Some(&resource), &[])?;
        let result = execute::call(
            &callback,
            &Value::Undefined,
            &[
                error.unwrap_or(Value::Null),
                Value::Number(pending.started.elapsed().as_secs_f64() * 1000.0),
                crate::modules::buffer_proto::make_buffer(&pending.payload),
            ],
        );
        crate::modules::async_hooks::resource_after(state, None, &[])?;
        crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
        result.map(|_| ())
    } else {
        crate::modules::async_hooks::resource_destroy(state, Some(&resource), &[])?;
        Ok(())
    }
}

/// Complete one wire PING acknowledgement.  The pending map is the sole
/// operation ledger, so an ACK can never invoke a callback twice or attach to
/// a different session after a transport alias is replaced.
pub(crate) fn complete_http2_ping(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
    payload: &[u8],
) -> Result<bool, VmError> {
    let pending = {
        let mut net = state.borrow_mut();
        let Some(entries) = net.net.http2_pings.get_mut(&socket_id) else {
            return Ok(false);
        };
        let Some(index) = entries.iter().position(|entry| entry.payload == payload) else {
            return Ok(false);
        };
        let pending = entries.remove(index);
        if entries.is_empty() {
            net.net.http2_pings.remove(&socket_id);
        }
        pending
    };
    invoke_ping_callback(state, pending, None)?;
    Ok(true)
}

/// Cancel all outstanding operations when the owning socket reaches its
/// terminal state.  This is shared by protocol errors and explicit destroy,
/// preserving Node's callback/error and async-resource lifecycle.
pub(crate) fn cancel_http2_pings(
    state: &Rc<RefCell<HostState>>,
    socket_id: u64,
) -> Result<(), VmError> {
    let pending = state
        .borrow_mut()
        .net
        .http2_pings
        .remove(&socket_id)
        .unwrap_or_default();
    for pending in pending {
        invoke_ping_callback(state, pending, Some(ping_error()))?;
    }
    Ok(())
}

/// RFC 9113 removed priority signalling from the HTTP/2 wire protocol.  Node
/// retains `stream.priority()` as a deprecated compatibility method, but its
/// current nghttp2 backend does not emit a PRIORITY event/frame for ordinary
/// calls.  Keep the method present and chainable on the Rust-owned stream
/// object; the protocol state remains the canonical transport representation.
fn stream_priority(receiver: Option<&Value>) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn decorate_client_session(socket: &Value, secure: bool) -> Result<(), VmError> {
    execute::set_property_in_place(socket, "request", session_capability("sessionRequest"));
    execute::set_property_in_place(socket, "close", session_capability("sessionClose"));
    for name in [
        "setNextStreamID",
        "setLocalWindowSize",
        "ping",
        "settings",
        "goaway",
        "altsvc",
    ] {
        execute::set_property_in_place(
            socket,
            name,
            host_api::bound_capability_with_arguments(
                crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
                vec![
                    Value::String("sessionMethod".into()),
                    Value::String(name.into()),
                ],
            ),
        );
    }
    execute::set_property_in_place(socket, "pendingSettingsAck", Value::Boolean(false));
    // Session settings are stable snapshots in Node.  Keep one object per
    // direction on the Rust-owned session identity so repeated property reads
    // preserve object identity even before the first peer SETTINGS frame.
    let local_settings = default_settings();
    let remote_settings = default_settings();
    execute::set_property_in_place(socket, "localSettings", local_settings);
    execute::set_property_in_place(socket, "remoteSettings", remote_settings);
    execute::set_property_in_place(socket, HTTP2_MAX_OUTSTANDING_PINGS, Value::Number(10.0));
    execute::set_property_in_place(
        socket,
        "state",
        host_api::object(vec![
            (
                "effectiveLocalWindowSize".into(),
                Value::Number(4_194_304.0),
            ),
            ("localWindowSize".into(), Value::Number(33_554_432.0)),
            ("remoteWindowSize".into(), Value::Number(65_535.0)),
            ("nextStreamID".into(), Value::Number(1.0)),
        ]),
    );
    execute::set_property_in_place(socket, HTTP2_SOCKET_SYMBOL, socket.clone());
    execute::set_property_in_place(
        socket,
        "alpnProtocol",
        Value::String(if secure { "h2" } else { "h2c" }.into()),
    );
    Ok(())
}

pub(crate) fn decorate_server_session(socket: &Value) -> Result<(), VmError> {
    decorate_client_session(socket, false)
}

pub(crate) const HTTP2_DIAG_CREATED: &str = "created";
pub(crate) const HTTP2_DIAG_START: &str = "start";
pub(crate) const HTTP2_DIAG_FINISH: &str = "finish";
pub(crate) const HTTP2_DIAG_CLOSE: &str = "close";
pub(crate) const HTTP2_DIAG_ERROR: &str = "error";

const HTTP2_DIAG_CREATED_PROP: &str = "\0quench:http2:diagnostics:created";
const HTTP2_DIAG_START_PROP: &str = "\0quench:http2:diagnostics:start";
const HTTP2_DIAG_FINISH_PROP: &str = "\0quench:http2:diagnostics:finish";
const HTTP2_DIAG_CLOSE_PROP: &str = "\0quench:http2:diagnostics:close";
const HTTP2_DIAG_ERROR_PROP: &str = "\0quench:http2:diagnostics:error";

/// Attach the observable stream family identity to a host-created stream.
/// The emitter methods remain host-owned, while the shared Duplex prototype
/// supplies the standard `instanceof` relationship used by diagnostics
/// consumers.  A private constructor record avoids mutating the global
/// `Duplex` constructor while retaining Node's concrete stream names.
pub(crate) fn decorate_http2_stream(state: &Rc<RefCell<HostState>>, stream: &Value, server: bool) {
    let constructor = host_api::object(vec![(
        "name".into(),
        Value::String(
            if server {
                "ServerHttp2Stream"
            } else {
                "ClientHttp2Stream"
            }
            .into(),
        ),
    )]);
    // The stream is retained by transport maps and may already have aliases;
    // define-property would publish a COW replacement that this host handle
    // cannot return. An in-place own slot keeps the concrete constructor name
    // visible through every diagnostic/event representative.
    let _ = execute::set_property_in_place(stream, "constructor", constructor);
    // Duplex exposes lifecycle accessors on its prototype.  HTTP/2 streams
    // need writable own state so close/destroy transitions remain observable
    // even when the inherited accessor has no setter.
    // These lifecycle slots are host-owned state. Define-property is an
    // ordinary COW operation and returns a replacement value, which this
    // borrowed host handle cannot publish back to its caller. In-place writes
    // keep the canonical stream identity visible through all aliases while
    // still shadowing Duplex's inherited accessors.
    for name in ["closed", "destroyed", "aborted"] {
        let _ = execute::set_property_in_place(stream, name, Value::Boolean(false));
    }
    let _ = execute::set_property_in_place(&stream, "bufferSize", Value::Number(0.0));
    let _ = execute::set_property_in_place(&stream, "writableEnded", Value::Boolean(false));
    let _ = execute::set_property_in_place(&stream, "writableFinished", Value::Boolean(false));
    // Node keeps a stable stream state view even though priority signalling is
    // deprecated.  Build it once with the defaults shared by client and
    // server streams so callers never observe an absent/null state object.
    let stream_state = host_api::object(vec![
        ("sumDependencyWeight".into(), Value::Number(0.0)),
        ("weight".into(), Value::Number(16.0)),
        ("localWindowSize".into(), Value::Number(65_535.0)),
        ("remoteWindowSize".into(), Value::Number(65_535.0)),
        ("localClose".into(), Value::Boolean(false)),
        ("remoteClose".into(), Value::Boolean(false)),
    ]);
    let _ = execute::set_property_in_place(&stream, "state", stream_state);
    let _ = execute::set_property_in_place(
        &stream,
        "priority",
        session_capability("streamPriority"),
    );
    // Set the shared Duplex prototype after host-owned fields are installed.
    // Prototype assignment may publish a copy-on-write replacement; doing it
    // first would leave subsequent in-place fields on the stale stream view.
    if let Some(module) = state.borrow().stream_module.clone() {
        let duplex = execute::get_property(&module, "Duplex");
        let prototype = execute::get_property(&duplex, "prototype");
        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
            let _ = execute::set_prototype_of(stream, &prototype);
        }
    }
}

fn http2_diag_name(server: bool, event: &str) -> String {
    format!(
        "http2.{}.stream.{}",
        if server { "server" } else { "client" },
        event
    )
}

/// Publish one of Node's built-in HTTP/2 stream diagnostics records.  The
/// one-shot marker belongs to the canonical stream object, not to an event
/// callback, so split TCP reads and repeated terminal frames cannot duplicate
/// an observable diagnostic.
pub(crate) fn publish_http2_stream_diagnostic(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    server: bool,
    event: &str,
    headers: Option<Value>,
    flags: Option<u8>,
    error: Option<Value>,
) -> Result<(), VmError> {
    // Host-side property updates may publish a COW representative while a
    // stream is retained by the transport map.  Resolve that replacement
    // before building the diagnostic message so observers see the same
    // `closed`/`destroyed` state that the lifecycle transition just wrote.
    let stream = execute::canonical_value(stream);
    let destroyed = execute::get_property(&stream, "destroyed");
    if event == HTTP2_DIAG_CLOSE {
        // Closing is observable on the diagnostic record itself, even when
        // the transport reached this boundary through a terminal DATA frame
        // rather than the JS `close()` capability.
        execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
        if matches!(destroyed, Value::Boolean(_)) {
            execute::set_property_in_place(&stream, "destroyed", destroyed);
        }
    }
    let marker = match event {
        HTTP2_DIAG_CREATED => HTTP2_DIAG_CREATED_PROP,
        HTTP2_DIAG_START => HTTP2_DIAG_START_PROP,
        HTTP2_DIAG_FINISH => HTTP2_DIAG_FINISH_PROP,
        HTTP2_DIAG_CLOSE => HTTP2_DIAG_CLOSE_PROP,
        HTTP2_DIAG_ERROR => HTTP2_DIAG_ERROR_PROP,
        _ => return Ok(()),
    };
    if matches!(execute::get_property(&stream, marker), Value::Boolean(true)) {
        return Ok(());
    }
    execute::set_property_in_place(&stream, marker, Value::Boolean(true));
    let mut message = vec![("stream".into(), stream.clone())];
    if let Some(headers) = headers {
        message.push(("headers".into(), headers));
    }
    if let Some(flags) = flags {
        message.push(("flags".into(), Value::Number(flags as f64)));
    }
    if let Some(error) = error {
        message.push(("error".into(), error));
    }
    crate::modules::diagnostics_channel::publish_named(
        state,
        &http2_diag_name(server, event),
        host_api::object(message),
    )
}

pub(crate) fn http2_diagnostic_headers(fields: &[(Vec<u8>, Vec<u8>)]) -> Value {
    let headers = host_api::object(Vec::new());
    for (name, value) in fields {
        let key = String::from_utf8_lossy(name).into_owned();
        let value = if key == ":status" {
            String::from_utf8_lossy(value)
                .parse::<f64>()
                .map(Value::Number)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(value).into_owned()))
        } else {
            Value::String(String::from_utf8_lossy(value).into_owned())
        };
        let previous = execute::get_property(&headers, &key);
        let next = match previous {
            Value::Undefined => value,
            Value::Array(_) => {
                let length = execute::get_property(&previous, "length");
                if let Value::Number(length) = length {
                    let _ = execute::set_property_in_place(&previous, &length.to_string(), value);
                }
                previous
            }
            other => host_api::array(vec![other, value]),
        };
        let _ = execute::set_property_in_place(&headers, &key, next);
    }
    // Populate first: the host object writer requires the ordinary object
    // shape while installing fields, after which the observable null
    // prototype can be applied for Node's header-record identity.
    let _ = execute::set_prototype_of(&headers, &Value::Null);
    headers
}

fn http2_binding_request_override() -> Option<Value> {
    // `internalBinding('http2')` may materialize a fresh namespace object for
    // each caller, but its Http2Session prototype is the shared native
    // identity.  Constructing the view here therefore observes the same
    // prototype patch without depending on a stale module-object snapshot.
    let binding = binding();
    let session = execute::get_property(&binding, "Http2Session");
    let prototype = execute::canonical_value(&execute::get_property(&session, "prototype"));
    let request = execute::get_property(&prototype, "request");
    quench_runtime::is_callable(&request).then_some(request)
}

fn binding_request_error(code: i64) -> Value {
    let (name, message) = match code {
        -509 => (
            "ERR_HTTP2_OUT_OF_STREAMS",
            "No stream ID is available because maximum stream ID has been reached",
        ),
        -501 => (
            "ERR_HTTP2_STREAM_SELF_DEPENDENCY",
            "A stream cannot depend on itself",
        ),
        _ => ("ERR_HTTP2_ERROR", ""),
    };
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    execute::set_property(error, "code", Value::String(name.into()))
}

fn stream_cancel_error(cause: Value) -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("The pending stream has been canceled".into())],
    );
    let error = execute::set_property(
        error,
        "code",
        Value::String("ERR_HTTP2_STREAM_CANCEL".into()),
    );
    execute::set_property(error, "cause", cause)
}

fn complete_binding_request(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
    receiver: Option<&Value>,
    stream_id: u32,
) -> Result<Value, VmError> {
    let Some(binding_request) = http2_binding_request_override() else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let result = execute::call(&binding_request, socket, &[])?;
    let Value::Number(code) = result else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let stream = receiver.cloned().unwrap_or(Value::Undefined);
    if !matches!(stream, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(Value::Undefined);
    }
    if let Some(socket_id) = crate::modules::net::net_id(socket) {
        let mut host = state.borrow_mut();
        if let Some(session) = host.net.http2_sessions.get_mut(&socket_id) {
            session.streams.remove(&stream_id);
        }
        host.net.http2_streams.remove(&(socket_id, stream_id));
    }
    execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
    execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
    execute::set_property_in_place(&stream, "writableEnded", Value::Boolean(true));
    execute::set_property_in_place(&stream, "writableFinished", Value::Boolean(true));
    execute::set_property_in_place(&stream, "\0quenchHttp2CloseEmitted", Value::Boolean(true));
    let code = code as i64;
    if code == -509 || code == -501 {
        let error = binding_request_error(code);
        publish_http2_stream_diagnostic(
            state,
            &stream,
            false,
            HTTP2_DIAG_ERROR,
            None,
            None,
            Some(error.clone()),
        )?;
        state.borrow_mut().net.pending_http2_events.push((
            stream.clone(),
            "error".into(),
            vec![error],
        ));
    } else {
        let error = nghttp_error(&[Value::Number(code as f64)])?;
        let cancel = stream_cancel_error(error.clone());
        state.borrow_mut().net.pending_http2_events.extend([
            (socket.clone(), "error".into(), vec![error]),
            (stream.clone(), "error".into(), vec![cancel]),
        ]);
    }
    state
        .borrow_mut()
        .net
        .pending_http2_events
        .push((stream, "close".into(), Vec::new()));
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn session_request(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let socket = receiver.cloned().unwrap_or(Value::Undefined);
    let Some(socket_id) = crate::modules::net::net_id(&socket) else {
        return Err(VmError::NotCallable);
    };
    // The native binding is an explicit seam used by Node's own
    // requestOnConnect tests.  Detect an overridden handle method before
    // queueing HEADERS; its result is consumed by stream.end() after the
    // caller has set the native return code.
    let binding_request = http2_binding_request_override();
    let headers = values.first().unwrap_or(&Value::Undefined);
    if !matches!(
        headers,
        Value::Undefined | Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_ARG_TYPE",
            "The \"headers\" argument must be of type object.".into(),
        ));
    }
    if let Some(options) = values
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        validate_request_options(options)?;
    }
    let mut fields = Vec::<(Vec<u8>, Vec<u8>)>::new();
    if matches!(headers, Value::Object(_) | Value::ObjectAlias(_)) {
        for key in execute::own_enumerable_keys(headers) {
            let value = execute::get_property(headers, &key);
            let text = execute::to_js_string(&value)?;
            fields.push((key.to_ascii_lowercase().into_bytes(), text.into_bytes()));
        }
    } else if let Value::Array(items) = headers {
        // Node accepts the legacy alternating `[name, value, ...]` header
        // form on ClientHttp2Session#request(). Treat it as a header record
        // rather than rejecting the array at the API boundary.
        let length = items.logical_len();
        let mut index = 0;
        while index + 1 < length {
            let name = execute::to_js_string(&execute::get_property(headers, &index.to_string()))?;
            let value =
                execute::to_js_string(&execute::get_property(headers, &(index + 1).to_string()))?;
            let wire_name = if name.starts_with(':') {
                name.to_ascii_lowercase()
            } else {
                name
            };
            fields.push((wire_name.into_bytes(), value.into_bytes()));
            index += 2;
        }
    }
    let method = fields
        .iter()
        .find(|(name, _)| name.as_slice() == b":method")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| b"GET".to_vec());
    let invalid_path = fields.iter().any(|(name, value)| {
        name.as_slice() == b":path" && value.iter().any(|byte| *byte <= 0x20 || *byte == 0x7f)
    });
    if method.as_slice() == b"CONNECT" {
        let authority = fields
            .iter()
            .any(|(name, _)| name.as_slice() == b":authority");
        let scheme = fields.iter().any(|(name, _)| name.as_slice() == b":scheme");
        let path = fields.iter().any(|(name, _)| name.as_slice() == b":path");
        if !authority {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_HTTP2_CONNECT_AUTHORITY",
                ":authority header is required for CONNECT requests".into(),
            ));
        }
        if scheme {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_HTTP2_CONNECT_SCHEME",
                "The :scheme header is forbidden for CONNECT requests".into(),
            ));
        }
        if path {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_HTTP2_CONNECT_PATH",
                "The :path header is forbidden for CONNECT requests".into(),
            ));
        }
    }
    if !fields.iter().any(|(name, _)| name.as_slice() == b":method") {
        fields.push((b":method".to_vec(), b"GET".to_vec()));
    }
    if !fields.iter().any(|(name, _)| name.as_slice() == b":path")
        && method.as_slice() != b"CONNECT"
    {
        fields.push((b":path".to_vec(), b"/".to_vec()));
    }
    if !fields.iter().any(|(name, _)| name.as_slice() == b":scheme")
        && method.as_slice() != b"CONNECT"
    {
        fields.push((b":scheme".to_vec(), b"http".to_vec()));
    }
    if !fields
        .iter()
        .any(|(name, _)| name.as_slice() == b":authority")
    {
        let host = match execute::get_property(&socket, "\0quench:http2-authority") {
            Value::String(authority) if !authority.is_empty() => authority,
            _ => match execute::get_property(&socket, "host") {
                Value::String(host) if !host.is_empty() => host,
                _ => "localhost".into(),
            },
        };
        fields.push((b":authority".to_vec(), host.into_bytes()));
    }
    let stream_id = match execute::get_property(&socket, "\0quench:http2-next-stream-id") {
        Value::Number(id) if id.is_finite() && id.fract() == 0.0 && id >= 1.0 => id as u32,
        _ => state
            .borrow()
            .net
            .http2_sessions
            .get(&socket_id)
            .and_then(|session| session.streams.keys().copied().max())
            .map(|id| id.saturating_add(2))
            .unwrap_or(1),
    };
    let block = {
        let mut host = state.borrow_mut();
        let session = host
            .net
            .http2_sessions
            .get_mut(&socket_id)
            .ok_or(VmError::NotCallable)?;
        session.streams.insert(
            stream_id,
            crate::modules::http2_protocol::Stream {
                state: crate::modules::http2_protocol::StreamState::Open,
                recv_window: 65_535,
                send_window: 65_535,
            },
        );
        session.encode_headers(
            &fields
                .iter()
                .map(|(name, value)| (name.as_slice(), value.as_slice()))
                .collect::<Vec<_>>(),
        )
    };
    // Client requests stay writable until `end()` (or an explicit
    // `endStream: true`) is observed. This is the stream contract needed for
    // POST bodies and for AbortSignal cancellation to reach the peer before
    // any terminal close event is surfaced.
    let has_abort_signal = values
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .is_some_and(|options| {
            matches!(
                execute::get_property(options, "signal"),
                Value::Object(_) | Value::ObjectAlias(_)
            )
        });
    let end_stream = match values
        .get(1)
        .map(|value| execute::get_property(value, "endStream"))
    {
        Some(Value::Boolean(value)) => value,
        // Keep an AbortSignal-backed request open until its cancellation can
        // reach the peer. Otherwise a header-only GET's END_STREAM is
        // observed first and the server closes with rstCode 0 before the
        // cancellation RST_STREAM arrives.
        _ if has_abort_signal => false,
        _ => !matches!(method.as_slice(), b"POST" | b"PUT" | b"PATCH"),
    };
    let frame = crate::modules::http2_protocol::Frame::new(
        crate::modules::http2_protocol::FrameType::Headers,
        // An explicit END_STREAM option permits a header-only request;
        // ordinary requests remain open for DATA frames and `.end()`.
        0x4 | u8::from(end_stream),
        stream_id,
        block,
    );
    let stream = crate::modules::events::new_emitter_object(state)?;
    execute::set_property_in_place(&stream, "\0quench:http2-socket", socket.clone());
    execute::set_property_in_place(
        &stream,
        "\0quench:http2-stream-id",
        Value::Number(stream_id as f64),
    );
    execute::set_property_in_place(&stream, "id", Value::Number(stream_id as f64));
    execute::set_property_in_place(&stream, "write", session_capability("streamWrite"));
    execute::set_property_in_place(&stream, "end", session_capability("streamEnd"));
    execute::set_property_in_place(&stream, "close", session_capability("streamClose"));
    execute::set_property_in_place(&stream, "destroy", session_capability("streamDestroy"));
    // Host-created requests bypass the ordinary Duplex constructor, so the
    // internal lifecycle hook must be installed explicitly for callers that
    // inspect or wrap `_destroy`.
    execute::set_property_in_place(&stream, "_destroy", session_capability("streamDestroy"));
    execute::set_property_in_place(&stream, "respond", session_capability("streamRespond"));
    execute::set_property_in_place(
        &stream,
        "pushStream",
        session_capability("streamPushStream"),
    );
    execute::set_property_in_place(
        &stream,
        "setEncoding",
        session_capability("streamSetEncoding"),
    );
    execute::set_property_in_place(&stream, "resume", session_capability("streamResume"));
    execute::set_property_in_place(&stream, "pause", session_capability("streamPause"));
    execute::set_property_in_place(&stream, "session", socket.clone());
    execute::set_property_in_place(&stream, "rstCode", Value::Number(0.0));
    execute::set_property_in_place(
        &stream,
        "\0quench:http2:end-stream",
        Value::Boolean(end_stream),
    );
    execute::set_property_in_place(
        &stream,
        "\0quench:http2-binding-request",
        Value::Boolean(binding_request.is_some()),
    );
    decorate_http2_stream(state, &stream, false);
    // Keep the deprecated compatibility method on the request's own shape;
    // this stream is returned before the transport creates its peer view.
    execute::set_property_in_place(&stream, "priority", session_capability("streamPriority"));
    if invalid_path {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "Stream closed with error code NGHTTP2_PROTOCOL_ERROR".into(),
            )],
        );
        let error = execute::set_property(
            error,
            "code",
            Value::String("ERR_HTTP2_STREAM_ERROR".into()),
        );
        execute::set_property_in_place(&stream, "rstCode", Value::Number(1.0));
        execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((stream.clone(), "error".into(), vec![error]));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((stream.clone(), "close".into(), Vec::new()));
        return Ok(stream);
    }
    if stream_id > 0x7fff_ffff {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "No stream ID is available because maximum stream ID has been reached".into(),
            )],
        );
        let error = execute::set_property(
            error,
            "code",
            Value::String("ERR_HTTP2_OUT_OF_STREAMS".into()),
        );
        execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((stream.clone(), "error".into(), vec![error]));
        return Ok(stream);
    }
    state
        .borrow_mut()
        .net
        .http2_streams
        .insert((socket_id, stream_id), stream.clone());
    let streams = match execute::get_property(&socket, "\0quench:http2-streams") {
        Value::Object(_) | Value::ObjectAlias(_) => {
            execute::get_property(&socket, "\0quench:http2-streams")
        }
        _ => {
            let map = host_api::object(Vec::new());
            execute::set_property_in_place(&socket, "\0quench:http2-streams", map.clone());
            map
        }
    };
    execute::set_property_in_place(&streams, &stream_id.to_string(), stream.clone());
    let diagnostic_headers = http2_diagnostic_headers(&fields);
    execute::set_property_in_place(
        &stream,
        "__quenchHttp2RequestDiagnostics",
        diagnostic_headers.clone(),
    );
    let request_diagnostics =
        match execute::get_property(&socket, "\0quench:http2-request-diagnostics-map") {
            Value::Object(_) | Value::ObjectAlias(_) => {
                execute::get_property(&socket, "\0quench:http2-request-diagnostics-map")
            }
            _ => {
                let map = host_api::object(Vec::new());
                execute::set_property_in_place(
                    &socket,
                    "\0quench:http2-request-diagnostics-map",
                    map.clone(),
                );
                map
            }
        };
    execute::set_property_in_place(
        &request_diagnostics,
        &stream_id.to_string(),
        diagnostic_headers.clone(),
    );
    // Consume an explicit setNextStreamID override once; subsequent requests
    // continue with the next client-initiated odd identifier.
    execute::set_property_in_place(
        &socket,
        "\0quench:http2-next-stream-id",
        Value::Number(stream_id.saturating_add(2) as f64),
    );
    let session_state = execute::get_property(&socket, "state");
    execute::set_property_in_place(
        &session_state,
        "nextStreamID",
        Value::Number(stream_id.saturating_add(2) as f64),
    );
    publish_http2_stream_diagnostic(
        state,
        &stream,
        false,
        HTTP2_DIAG_CREATED,
        Some(diagnostic_headers.clone()),
        None,
        None,
    )?;
    publish_http2_stream_diagnostic(
        state,
        &stream,
        false,
        HTTP2_DIAG_START,
        Some(diagnostic_headers),
        None,
        None,
    )?;
    // A destroyed ClientHttp2Session still returns a request-shaped stream,
    // but it must fail on the next loop turn with the session error.  The
    // transport entry remains in the registry for event routing, so handle
    // this terminal state before attempting to submit a frame to the closed
    // socket (where `socket.write()` intentionally returns false).
    if matches!(
        execute::get_property(&socket, "destroyed"),
        Value::Boolean(true)
    ) {
        let stream = execute::canonical_value(&stream);
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("The session has been destroyed".into())],
        );
        let error = execute::set_property(
            error,
            "code",
            Value::String("ERR_HTTP2_INVALID_SESSION".into()),
        );
        execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
        execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
        execute::set_property_in_place(&stream, "__quenchHttp2CloseEmitted", Value::Boolean(true));
        let mut host = state.borrow_mut();
        host.net
            .pending_http2_events
            .push((stream.clone(), "error".into(), vec![error]));
        host.net
            .pending_http2_events
            .push((stream.clone(), "close".into(), Vec::new()));
        return Ok(stream);
    }
    if let Some(options) = values
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let signal = execute::get_property(options, "signal");
        if matches!(signal, Value::Object(_) | Value::ObjectAlias(_)) {
            if matches!(
                execute::get_property(&signal, "aborted"),
                Value::Boolean(true)
            ) {
                stream_abort(state, std::slice::from_ref(&stream))?;
            } else {
                let listener = host_api::bound_capability_with_arguments(
                    crate::host::capability_ref(crate::registry::SPEC_INTERNAL_HTTP2_UTIL),
                    vec![Value::String("streamAbort".into()), stream.clone()],
                );
                let listener_options =
                    host_api::object(vec![("once".into(), Value::Boolean(true))]);
                crate::modules::event_target::add_event_listener(
                    state,
                    Some(&signal),
                    &[Value::String("abort".into()), listener, listener_options],
                )?;
            }
        }
    }
    // An already-aborted signal destroys the request synchronously.  Do not
    // submit its HEADERS after the cancellation RST_STREAM: the request
    // state machine has already reached a terminal wire state.
    if binding_request.is_none()
        && !matches!(
            execute::get_property(&socket, "destroyed"),
            Value::Boolean(true)
        )
        && !matches!(
            execute::get_property(&stream, "destroyed"),
            Value::Boolean(true)
        )
    {
        // A signal can be aborted later in this same JavaScript turn. Keep
        // the initial HEADERS in the host queue until the next pump tick so
        // synchronous cancellation wins before a request becomes visible to
        // the peer. Requests without a signal retain immediate submission.
        if has_abort_signal {
            state
                .borrow_mut()
                .net
                .pending_writes
                .push((socket.clone(), frame.encode()));
        } else {
            write_http2_frame(&socket, &frame)?;
        }
    }
    Ok(stream)
}

fn validate_request_options(options: &Value) -> Result<(), VmError> {
    for (name, expected) in [
        ("endStream", "boolean"),
        ("parent", "number"),
        ("exclusive", "boolean"),
        ("silent", "boolean"),
    ] {
        if !execute::has_own_property(options, name) {
            continue;
        }
        let value = execute::get_property(options, name);
        let valid = match expected {
            "boolean" => matches!(value, Value::Boolean(_)),
            "number" => matches!(value, Value::Number(_)),
            _ => false,
        };
        if !valid {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"{name}\" option must be of type {expected}.{}",
                    crate::modules::util::invalid_arg_received(&value)
                ),
            ));
        }
    }
    Ok(())
}

fn stream_socket(receiver: Option<&Value>) -> Result<(Value, u32), VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    let socket = execute::get_property(stream, "\0quench:http2-socket");
    let stream_id = match execute::get_property(stream, "\0quench:http2-stream-id") {
        Value::Number(id) if id.is_finite() && id > 0.0 => id as u32,
        _ => return Err(VmError::NotCallable),
    };
    Ok((socket, stream_id))
}

fn stream_set_encoding(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    let encoding = values
        .first()
        .map(execute::to_js_string)
        .transpose()?
        .unwrap_or_else(|| "utf8".into())
        .to_ascii_lowercase();
    execute::set_property_in_place(stream, "encoding", Value::String(encoding.clone()));
    // A request stream can be observed through a canonical stream object
    // created when the peer's response headers arrive. Keep the encoding on
    // that shared host record as well, so data dispatch never falls back to
    // byte-array coercion merely because the VM exposed a distinct wrapper.
    let socket = execute::get_property(stream, "\0quench:http2-socket");
    let stream_id = execute::get_property(stream, "\0quench:http2-stream-id");
    if let (Some(socket_id), Value::Number(id)) = (crate::modules::net::net_id(&socket), stream_id)
    {
        if let Some(canonical) = state
            .borrow()
            .net
            .http2_streams
            .get(&(socket_id, id as u32))
            .cloned()
        {
            execute::set_property_in_place(&canonical, "encoding", Value::String(encoding));
        }
    }
    Ok(stream.clone())
}

fn write_http2_frame(
    socket: &Value,
    frame: &crate::modules::http2_protocol::Frame,
) -> Result<(), VmError> {
    let write = execute::get_property(socket, "write");
    if !quench_runtime::is_callable(&write) {
        return Err(VmError::NotCallable);
    }
    execute::call(
        &write,
        socket,
        &[crate::modules::buffer_proto::make_buffer(&frame.encode())],
    )?;
    Ok(())
}

fn stream_write(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let (socket, stream_id) = stream_socket(receiver)?;
    let body = values.first().unwrap_or(&Value::Undefined);
    let bytes = crate::modules::crypto::bytes_from_value(body)
        .or_else(|| {
            matches!(body, Value::String(_) | Value::StringUnits(_))
                .then(|| execute::to_js_string(body).ok())
                .flatten()
                .map(String::into_bytes)
        })
        .unwrap_or_default();
    let body_len = bytes.len();
    let frame = crate::modules::http2_protocol::Frame::new(
        crate::modules::http2_protocol::FrameType::Data,
        0,
        stream_id,
        bytes,
    );
    if !matches!(
        execute::get_property(&socket, "destroyed"),
        Value::Boolean(true)
    ) {
        write_http2_frame(&socket, &frame)?;
    }
    if let Some(receiver) = receiver {
        let stream = execute::canonical_value(receiver);
        let current = match execute::get_property(&stream, "bufferSize") {
            Value::Number(size) if size.is_finite() && size >= 0.0 => size,
            _ => 0.0,
        };
        execute::set_property_in_place(
            &stream,
            "bufferSize",
            Value::Number(current + body_len as f64),
        );
        if let Some(callback) = values
            .get(1)
            .filter(|value| quench_runtime::is_callable(value))
        {
            execute::call(callback, &stream, &[])?;
        }
    }
    Ok(Value::Boolean(true))
}

fn stream_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let (socket, stream_id) = stream_socket(receiver)?;
    if matches!(
        receiver.map(|stream| execute::get_property(stream, "\0quench:http2-binding-request")),
        Some(Value::Boolean(true))
    ) {
        return complete_binding_request(state, &socket, receiver, stream_id);
    }
    // A non-clean `close(code)` still enters Writable's destroy lifecycle
    // when the request is ended. This is intentionally resolved here rather
    // than by fabricating a filename-specific result: callers may install an
    // `_destroy` wrapper between the synchronous close and `.end()`.
    if receiver.is_some_and(|stream| {
        matches!(execute::get_property(stream, "closed"), Value::Boolean(true))
    }) {
        if let Some(stream) = receiver {
            if !matches!(execute::get_property(stream, "destroyed"), Value::Boolean(true)) {
                let destroy = execute::get_property(stream, "_destroy");
                if quench_runtime::is_callable(&destroy) {
                    execute::call(&destroy, stream, &[Value::Null])?;
                }
                execute::set_property_in_place(stream, "destroyed", Value::Boolean(true));
                let canonical = execute::canonical_value(stream);
                execute::set_property_in_place(&canonical, "destroyed", Value::Boolean(true));
            }
        }
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let body = values.first().unwrap_or(&Value::Undefined);
    let bytes = crate::modules::crypto::bytes_from_value(body)
        .or_else(|| {
            matches!(body, Value::String(_) | Value::StringUnits(_))
                .then(|| execute::to_js_string(body).ok())
                .flatten()
                .map(String::into_bytes)
        })
        .unwrap_or_default();
    let body_len = bytes.len();
    // `request()` submits END_STREAM by default.  `request().end()` is still
    // a common spelling for that header-only request, but sending a second
    // empty DATA frame would produce duplicate end/close observations and an
    // invalid wire transition.  Only an explicitly open stream needs DATA.
    if bytes.is_empty()
        && matches!(
            receiver.map(|stream| execute::get_property(stream, "\0quench:http2:end-stream")),
            Some(Value::Boolean(true))
        )
    {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let frame = crate::modules::http2_protocol::Frame::new(
        crate::modules::http2_protocol::FrameType::Data,
        0x1,
        stream_id,
        bytes,
    );
    if let Some(session) = state
        .borrow_mut()
        .net
        .http2_sessions
        .get_mut(&crate::modules::net::net_id(&socket).unwrap_or_default())
    {
        if let Some(protocol_stream) = session.streams.get_mut(&stream_id) {
            protocol_stream.state = match protocol_stream.state {
                crate::modules::http2_protocol::StreamState::HalfClosedRemote => {
                    crate::modules::http2_protocol::StreamState::Closed
                }
                crate::modules::http2_protocol::StreamState::Closed => {
                    crate::modules::http2_protocol::StreamState::Closed
                }
                _ => crate::modules::http2_protocol::StreamState::HalfClosedLocal,
            };
        }
    }
    // Match Node's writable-stream ordering: `end(chunk)` queues its final
    // DATA frame, allowing writes made later in the same callback turn to be
    // flushed first. The host pump drains this queue in FIFO order on the
    // next transport tick.
    if !matches!(
        execute::get_property(&socket, "destroyed"),
        Value::Boolean(true)
    ) {
        state
            .borrow_mut()
            .net
            .pending_writes
            .push((socket.clone(), frame.encode()));
    }
    let stream = receiver.cloned().unwrap_or(Value::Undefined);
    execute::set_property_in_place(&stream, "writableEnded", Value::Boolean(true));
    execute::set_property_in_place(&stream, "writableFinished", Value::Boolean(true));
    if let Some(receiver) = receiver {
        let stream = execute::canonical_value(receiver);
        let current = match execute::get_property(&stream, "bufferSize") {
            Value::Number(size) if size.is_finite() && size >= 0.0 => size,
            _ => 0.0,
        };
        execute::set_property_in_place(
            &stream,
            "bufferSize",
            Value::Number(current + body_len as f64),
        );
        if let Some(callback) = values
            .get(1)
            .filter(|value| quench_runtime::is_callable(value))
        {
            execute::call(callback, &stream, &[])?;
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// Build the `(Http2ServerRequest, Http2ServerResponse)` pair delivered by
/// `http2.createServer`'s compatibility callback.  The wire parser already
/// owns the canonical stream and transport socket; these two objects are
/// only the public request/response views over those identities.
pub(crate) fn compat_server_request_response(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    headers: &Value,
) -> Result<(Value, Value), VmError> {
    let socket = execute::get_property(stream, "\0quench:http2-socket");
    let mut request = crate::modules::events::new_emitter_object(state)?;
    let method = execute::get_property(headers, ":method");
    let path = execute::get_property(headers, ":path");
    for (name, value) in [
        ("method", method),
        (
            "url",
            if matches!(path, Value::Undefined) {
                Value::String("/".into())
            } else {
                path
            },
        ),
        ("httpVersion", Value::String("2.0".into())),
        ("httpVersionMajor", Value::Number(2.0)),
        ("httpVersionMinor", Value::Number(0.0)),
        ("headers", headers.clone()),
        ("trailers", host_api::object(Vec::new())),
        ("rawTrailers", host_api::array(Vec::new())),
        ("socket", socket.clone()),
        ("connection", socket.clone()),
        ("complete", Value::Boolean(false)),
        ("aborted", Value::Boolean(false)),
        ("destroyed", Value::Boolean(false)),
        ("readable", Value::Boolean(true)),
    ] {
        execute::set_property_in_place(&request, name, value);
    }
    for (name, method) in [
        ("pause", execute::get_property(stream, "pause")),
        ("resume", execute::get_property(stream, "resume")),
        ("destroy", execute::get_property(stream, "destroy")),
    ] {
        execute::set_property_in_place(&request, name, method);
    }

    let mut response = crate::modules::events::new_emitter_object(state)?;
    for (name, value) in [
        ("socket", socket.clone()),
        ("connection", socket),
        ("req", request.clone()),
        ("statusCode", Value::Number(200.0)),
        ("headersSent", Value::Boolean(false)),
        ("finished", Value::Boolean(false)),
        ("writableEnded", Value::Boolean(false)),
        ("destroyed", Value::Boolean(false)),
        ("closed", Value::Boolean(false)),
        // Node exposes the underlying ServerHttp2Stream through the
        // compatibility response. Keep this as the same identity used by
        // the host-only bridge rather than manufacturing a second wrapper.
        ("stream", stream.clone()),
        ("\0quench:http2-compat-stream", stream.clone()),
    ] {
        execute::set_property_in_place(&response, name, value);
    }
    for (name, method) in [
        ("writeHead", http2_capability("compatResponseWriteHead")),
        ("write", http2_capability("compatResponseWrite")),
        ("end", http2_capability("compatResponseEnd")),
        ("destroy", http2_capability("compatResponseDestroy")),
    ] {
        execute::set_property_in_place(&response, name, method);
    }
    Ok((request, response))
}

fn compat_response_stream(receiver: Option<&Value>) -> Result<Value, VmError> {
    let response = receiver.ok_or(VmError::NotCallable)?;
    let stream = execute::get_property(response, "\0quench:http2-compat-stream");
    if matches!(stream, Value::Object(_) | Value::ObjectAlias(_)) {
        Ok(stream)
    } else {
        Err(VmError::NotCallable)
    }
}

fn compat_response_write_head(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let stream = compat_response_stream(receiver)?;
    let status = values.first().cloned().unwrap_or(Value::Number(200.0));
    let headers = match values.get(1) {
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            // Do not add the response pseudo-header to the request's own
            // header object when the caller passed a mutable map.
            host_api::object(
                execute::own_enumerable_keys(&values[1])
                    .into_iter()
                    .map(|key| {
                        let value = execute::get_property(&values[1], &key);
                        (key, value)
                    })
                    .collect(),
            )
        }
        _ => host_api::object(Vec::new()),
    };
    execute::set_property_in_place(&headers, ":status", status);
    stream_respond(state, Some(&stream), &[headers])?;
    if let Some(response) = receiver {
        execute::set_property_in_place(response, "headersSent", Value::Boolean(true));
        execute::set_property_in_place(
            response,
            "statusCode",
            values.first().cloned().unwrap_or(Value::Number(200.0)),
        );
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn compat_response_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let stream = compat_response_stream(receiver)?;
    stream_write(state, Some(&stream), values)?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn compat_response_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let stream = compat_response_stream(receiver)?;
    stream_end(state, Some(&stream), values)?;
    if let Some(response) = receiver {
        execute::set_property_in_place(response, "finished", Value::Boolean(true));
        execute::set_property_in_place(response, "writableEnded", Value::Boolean(true));
        execute::set_property_in_place(response, "closed", Value::Boolean(true));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((response.clone(), "finish".into(), Vec::new()));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn compat_response_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let stream = compat_response_stream(receiver)?;
    stream_destroy(state, Some(&stream), values)?;
    if let Some(response) = receiver {
        execute::set_property_in_place(response, "destroyed", Value::Boolean(true));
        execute::set_property_in_place(response, "closed", Value::Boolean(true));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn stream_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let (socket, stream_id) = stream_socket(receiver)?;
    if matches!(
        receiver.map(|stream| execute::get_property(stream, "closed")),
        Some(Value::Boolean(true))
    ) {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let code = match values.first().unwrap_or(&Value::Undefined) {
        Value::Undefined => 0,
        Value::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= 0.0
                && *value <= u32::MAX as f64 =>
        {
            *value as u32
        }
        Value::Number(value) => {
            return Err(coded_error(
                quench_runtime::ops::Builtin::RangeError,
                "ERR_OUT_OF_RANGE",
                format!(
                    "The value of \"code\" is out of range. It must be >= 0 && <= {}. Received {}",
                    u32::MAX,
                    value
                ),
            ));
        }
        value => {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"code\" argument must be of type number.{}",
                    crate::modules::util::invalid_arg_received(value)
                ),
            ));
        }
    };
    if let Some(callback) = values.get(1) {
        if !quench_runtime::is_callable(callback) {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"callback\" argument must be of type function.{}",
                    crate::modules::util::invalid_arg_received(callback)
                ),
            ));
        }
    }
    let frame = crate::modules::http2_protocol::Frame::new(
        crate::modules::http2_protocol::FrameType::RstStream,
        0,
        stream_id,
        code.to_be_bytes().to_vec(),
    );
    if !matches!(
        execute::get_property(&socket, "destroyed"),
        Value::Boolean(true)
    ) {
        write_http2_frame(&socket, &frame)?;
    }
    if let Some(stream) = receiver {
        execute::set_property_in_place(stream, "rstCode", Value::Number(code as f64));
        execute::set_property_in_place(stream, "closed", Value::Boolean(true));
        if code != 0 {
            let message = format!(
                "Stream closed with error code {}",
                crate::modules::http2_facts::error_name(code).unwrap_or("UNKNOWN_ERROR")
            );
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(message)],
            );
            let error = execute::set_property(
                error,
                "code",
                Value::String("ERR_HTTP2_STREAM_ERROR".into()),
            );
            state.borrow_mut().net.pending_http2_events.push((
                stream.clone(),
                "error".into(),
                vec![error],
            ));
        }
        if let Some(callback) = values.get(1) {
            execute::call(callback, stream, &[])?;
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn stream_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?.clone();
    let stream = execute::canonical_value(&receiver);
    let (socket, stream_id) = stream_socket(Some(&stream))?;
    let error = values
        .first()
        .filter(|value| !matches!(value, Value::Undefined | Value::Null))
        .cloned();
    let prior_rst_code = match execute::get_property(&receiver, "rstCode") {
        Value::Number(code)
            if code.is_finite() && code.fract() == 0.0 && code > 0.0
                && code <= u32::MAX as f64 => Some(code as u32),
        _ => None,
    };
    let code = if error
        .as_ref()
        .is_some_and(|value| matches!(execute::get_property(value, "code"), Value::String(code) if code == "ABORT_ERR"))
    {
        8_u32 // NGHTTP2_CANCEL
    } else if error.is_none() {
        prior_rst_code.unwrap_or(2_u32) // NGHTTP2_INTERNAL_ERROR
    } else {
        2_u32 // NGHTTP2_INTERNAL_ERROR
    };
    let already_reset = matches!(
        execute::get_property(&stream, "closed"),
        Value::Boolean(true)
    ) && matches!(
        execute::get_property(&stream, "rstCode"),
        Value::Number(code)
            if code.is_finite() && code.fract() == 0.0 && code > 0.0
    );
    if let Some(socket_id) = crate::modules::net::net_id(&socket) {
        let mapped = state
            .borrow()
            .net
            .http2_streams
            .get(&(socket_id, stream_id))
            .cloned();
        state
            .borrow_mut()
            .net
            .http2_reset_codes
            .insert((socket_id, stream_id), code);
        if let Some(mapped) = mapped {
            execute::set_property_in_place(&mapped, "rstCode", Value::Number(code as f64));
            execute::set_property_in_place(&mapped, "closed", Value::Boolean(true));
            execute::set_property_in_place(&mapped, "destroyed", Value::Boolean(error.is_some()));
        }
        // Drop any not-yet-flushed frames for this terminal stream. This is
        // what makes same-turn AbortSignal cancellation win over the queued
        // request HEADERS (and also prevents a queued final DATA frame from
        // overtaking the RST_STREAM).
        state
            .borrow_mut()
            .net
            .pending_writes
            .retain(|(queued, bytes)| {
                if crate::modules::net::net_id(queued) != Some(socket_id) {
                    return true;
                }
                match crate::modules::http2_protocol::FrameHeader::decode(bytes) {
                    Ok(Some(header)) => header.stream_id != stream_id,
                    _ => true,
                }
            });
    }
    execute::set_property_in_place(&stream, "rstCode", Value::Number(code as f64));
    execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
    execute::set_property_in_place(&stream, "destroyed", Value::Boolean(error.is_some()));
    // Preserve the public receiver's lifecycle fields when a property write
    // promotes a copy-on-write representative. Abort-before-connect returns
    // this original object synchronously, so callers must observe the same
    // `aborted`/`destroyed` facts before the canonical transport alias is
    // revisited by the next pump tick.
    let _ = execute::define_property(
        receiver.clone(),
        "aborted",
        host_api::object(vec![
            ("value".into(), Value::Boolean(false)),
            ("writable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    );
    execute::set_property_in_place(&receiver, "destroyed", Value::Boolean(error.is_some()));
    execute::set_property_in_place(&receiver, "rstCode", Value::Number(code as f64));
    let is_server = matches!(
        execute::get_property(&socket, crate::modules::http2_protocol::SERVER_MARKER),
        Value::Boolean(true)
    );
    if let Some(error) = error.clone() {
        publish_http2_stream_diagnostic(
            state,
            &stream,
            is_server,
            HTTP2_DIAG_ERROR,
            None,
            None,
            Some(error.clone()),
        )?;
        // Destruction is observable on a later event-loop turn, allowing the
        // usual `destroy(error); stream.on('error', ...)` ordering.
        state.borrow_mut().net.pending_http2_events.push((
            receiver.clone(),
            "error".into(),
            vec![error],
        ));
    }
    publish_http2_stream_diagnostic(
        state,
        &stream,
        is_server,
        HTTP2_DIAG_CLOSE,
        None,
        None,
        None,
    )?;
    if !matches!(
        execute::get_property(&stream, "__quenchHttp2CloseEmitted"),
        Value::Boolean(true)
    ) {
        execute::set_property_in_place(&stream, "__quenchHttp2CloseEmitted", Value::Boolean(true));
        state.borrow_mut().net.pending_http2_events.push((
            stream.clone(),
            "close".into(),
            Vec::new(),
        ));
    }
    if !already_reset {
        let frame = crate::modules::http2_protocol::Frame::new(
            crate::modules::http2_protocol::FrameType::RstStream,
            0,
            stream_id,
            code.to_be_bytes().to_vec(),
        );
        write_http2_frame(&socket, &frame)?;
    }
    Ok(receiver)
}

fn stream_push_stream(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let parent = receiver.ok_or(VmError::NotCallable)?;
    let (socket, parent_id) = stream_socket(Some(parent))?;
    let headers = values.first().unwrap_or(&Value::Undefined);
    if !matches!(
        headers,
        Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
    ) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_ARG_TYPE",
            "The \"headers\" argument must be of type object.".into(),
        ));
    }
    let mut fields = Vec::new();
    if matches!(headers, Value::Object(_) | Value::ObjectAlias(_)) {
        for key in execute::own_enumerable_keys(headers) {
            fields.push((
                key.to_ascii_lowercase().into_bytes(),
                execute::to_js_string(&execute::get_property(headers, &key))?.into_bytes(),
            ));
        }
    }
    for (name, value) in [
        (b":method".to_vec(), b"GET".to_vec()),
        (b":path".to_vec(), b"/".to_vec()),
        (b":scheme".to_vec(), b"http".to_vec()),
    ] {
        if !fields.iter().any(|(key, _)| *key == name) {
            fields.push((name, value));
        }
    }
    if !fields
        .iter()
        .any(|(key, _)| key.as_slice() == b":authority")
    {
        let authority = match execute::get_property(parent, "__quenchHttp2RequestDiagnostics") {
            Value::Object(_) | Value::ObjectAlias(_) => {
                execute::to_js_string(&execute::get_property(
                    &execute::get_property(parent, "__quenchHttp2RequestDiagnostics"),
                    ":authority",
                ))
                .unwrap_or_else(|_| "localhost".into())
            }
            _ => {
                let map = execute::get_property(&socket, "\0quench:http2-request-diagnostics-map");
                let request = execute::get_property(&map, &parent_id.to_string());
                execute::to_js_string(&execute::get_property(&request, ":authority"))
                    .unwrap_or_else(|_| "localhost".into())
            }
        };
        fields.push((b":authority".to_vec(), authority.into_bytes()));
    }
    // Header records preserve wire/creation order.  Node emits the request
    // pseudo-headers first (`:method`, `:authority`, `:scheme`, `:path`),
    // followed by ordinary push headers; keeping that order also makes the
    // diagnostics object deterministic across HPACK table state.
    let mut ordered = Vec::with_capacity(fields.len());
    for name in [":method", ":authority", ":scheme", ":path"] {
        if let Some((_, value)) = fields
            .iter()
            .find(|(key, _)| key.as_slice() == name.as_bytes())
        {
            ordered.push((name.as_bytes().to_vec(), value.clone()));
        }
    }
    ordered.extend(fields.into_iter().filter(|(key, _)| {
        !matches!(
            key.as_slice(),
            b":method" | b":authority" | b":scheme" | b":path"
        )
    }));
    fields = ordered;
    let promised_id = state
        .borrow()
        .net
        .http2_sessions
        .get(&crate::modules::net::net_id(&socket).ok_or(VmError::NotCallable)?)
        .and_then(|session| session.streams.keys().copied().max())
        // Server push stream identifiers are even-numbered.  The parent
        // request is normally odd, so choosing `max + 2` would accidentally
        // create another client-style odd stream and lose push lifecycle
        // semantics on the receiving session.
        .map(|id| {
            let next = id.saturating_add(1).max(2);
            if next % 2 == 0 {
                next
            } else {
                next.saturating_add(1)
            }
        })
        .unwrap_or(2);
    let block = {
        let mut host = state.borrow_mut();
        let socket_id = crate::modules::net::net_id(&socket).ok_or(VmError::NotCallable)?;
        let session = host
            .net
            .http2_sessions
            .get_mut(&socket_id)
            .ok_or(VmError::NotCallable)?;
        session.streams.insert(
            promised_id,
            crate::modules::http2_protocol::Stream {
                state: crate::modules::http2_protocol::StreamState::Open,
                recv_window: 65_535,
                send_window: 65_535,
            },
        );
        session.encode_headers(
            &fields
                .iter()
                .map(|(name, value)| (name.as_slice(), value.as_slice()))
                .collect::<Vec<_>>(),
        )
    };
    let mut payload = promised_id.to_be_bytes().to_vec();
    payload.extend_from_slice(&block);
    let frame = crate::modules::http2_protocol::Frame::new(
        crate::modules::http2_protocol::FrameType::PushPromise,
        0x4,
        parent_id,
        payload,
    );
    let stream = crate::modules::events::new_emitter_object(state)?;
    execute::set_property_in_place(&stream, "\0quench:http2-socket", socket.clone());
    execute::set_property_in_place(
        &stream,
        "\0quench:http2-stream-id",
        Value::Number(promised_id as f64),
    );
    execute::set_property_in_place(&stream, "id", Value::Number(promised_id as f64));
    for (name, capability) in [
        ("write", "streamWrite"),
        ("end", "streamEnd"),
        ("close", "streamClose"),
        ("destroy", "streamDestroy"),
        ("respond", "streamRespond"),
        ("setEncoding", "streamSetEncoding"),
    ] {
        execute::set_property_in_place(&stream, name, session_capability(capability));
    }
    execute::set_property_in_place(&stream, "session", socket.clone());
    decorate_http2_stream(state, &stream, true);
    let diagnostics_headers = http2_diagnostic_headers(&fields);
    publish_http2_stream_diagnostic(
        state,
        &stream,
        true,
        HTTP2_DIAG_CREATED,
        Some(diagnostics_headers.clone()),
        None,
        None,
    )?;
    publish_http2_stream_diagnostic(
        state,
        &stream,
        true,
        HTTP2_DIAG_START,
        Some(diagnostics_headers),
        None,
        None,
    )?;
    let socket_id = crate::modules::net::net_id(&socket).ok_or(VmError::NotCallable)?;
    state
        .borrow_mut()
        .net
        .http2_streams
        .insert((socket_id, promised_id), stream.clone());
    write_http2_frame(&socket, &frame)?;
    if let Some(callback) = values
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
    {
        // Node's pushStream callback is error-first.  The PUSH_PROMISE must
        // be queued before user code can respond on the promised stream;
        // otherwise response HEADERS can overtake the promise on the wire
        // and the peer creates the stream before its `stream` notification.
        execute::call(callback, &Value::Undefined, &[Value::Null, stream.clone()])?;
    }
    Ok(stream)
}

fn stream_abort(state: &Rc<RefCell<HostState>>, values: &[Value]) -> Result<Value, VmError> {
    let Some(stream) = values.first() else {
        return Ok(Value::Undefined);
    };
    let result = stream_destroy(state, Some(stream), &[abort_error()]);
    result
}

fn abort_error() -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("The operation was aborted".into())],
    );
    let error = execute::set_property(error, "name", Value::String("AbortError".into()));
    execute::set_property(error, "code", Value::String("ABORT_ERR".into()))
}

fn stream_respond(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let (socket, stream_id) = stream_socket(receiver)?;
    // `ServerHttp2Stream#respond()` defaults its header map to an empty
    // object.  Keep the default at this API boundary so callers (including
    // the diagnostics-channel HTTP/2 fixtures) do not need to manufacture a
    // placeholder object merely to send the standard response headers.
    let default_headers = host_api::object(Vec::new());
    let headers = match values.first() {
        None | Some(Value::Undefined) => &default_headers,
        Some(value) => value,
    };
    if !matches!(headers, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_ARG_TYPE",
            "The \"headers\" argument must be of type object.".into(),
        ));
    }
    let mut fields = Vec::new();
    for key in execute::own_enumerable_keys(headers) {
        let value = execute::to_js_string(&execute::get_property(headers, &key))?;
        fields.push((key.to_ascii_lowercase().into_bytes(), value.into_bytes()));
    }
    if !fields.iter().any(|(name, _)| name.as_slice() == b":status") {
        fields.push((b":status".to_vec(), b"200".to_vec()));
    }
    // Node's HTTP/2 server adds a Date header by default when responding.
    // Keep this host-owned response fact in the encoded header block so
    // clients observe the same shape as the HTTP/1 response path.
    if !fields.iter().any(|(name, _)| name.as_slice() == b"date") {
        fields.push((b"date".to_vec(), b"Thu, 01 Jan 1970 00:00:00 GMT".to_vec()));
    }
    let block = {
        let mut host = state.borrow_mut();
        let id = crate::modules::net::net_id(&socket).ok_or(VmError::NotCallable)?;
        let session = host
            .net
            .http2_sessions
            .get_mut(&id)
            .ok_or(VmError::NotCallable)?;
        session.encode_headers(
            &fields
                .iter()
                .map(|(name, value)| (name.as_slice(), value.as_slice()))
                .collect::<Vec<_>>(),
        )
    };
    write_http2_frame(
        &socket,
        &crate::modules::http2_protocol::Frame::new(
            crate::modules::http2_protocol::FrameType::Headers,
            0x4,
            stream_id,
            block,
        ),
    )?;
    let is_server = matches!(
        execute::get_property(&socket, crate::modules::http2_protocol::SERVER_MARKER),
        Value::Boolean(true)
    );
    if is_server {
        decorate_http2_stream(state, receiver.unwrap_or(&Value::Undefined), true);
        publish_http2_stream_diagnostic(
            state,
            receiver.unwrap_or(&Value::Undefined),
            true,
            HTTP2_DIAG_FINISH,
            Some(http2_diagnostic_headers(&fields)),
            Some(4),
            None,
        )?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn session_close(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _values: &[Value],
) -> Result<Value, VmError> {
    let socket = receiver.ok_or(VmError::NotCallable)?;
    // A graceful ClientHttp2Session#close() rejects pending streams with the
    // GOAWAY-specific error.  Preserve that intent on the transport before
    // the shared socket teardown path runs; destroy() continues to use the
    // cancellation error.
    execute::set_property_in_place(
        socket,
        "\0quench:http2-close-requested",
        Value::Boolean(true),
    );
    let destroy = execute::get_property(socket, "destroy");
    if quench_runtime::is_callable(&destroy) {
        execute::call(&destroy, socket, &[])?;
    }
    Ok(socket.clone())
}

fn session_invalid_method(receiver: Option<&Value>) -> Result<Value, VmError> {
    let socket = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(socket, "destroyed"),
        Value::Boolean(true)
    ) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::Error,
            "ERR_HTTP2_INVALID_SESSION",
            "The session has been destroyed".into(),
        ));
    }
    Ok(Value::Undefined)
}

/// Validate and apply the small set of ClientHttp2Session controls whose
/// semantics are independent of the native nghttp2 backend.  Keeping these
/// checks in one capability gives every session wrapper the same argument and
/// range behavior while leaving wire-specific operations to the canonical
/// session state machine.
fn session_altsvc(
    state: &Rc<RefCell<HostState>>,
    socket: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    let alt = match args.first().unwrap_or(&Value::Undefined) {
        Value::String(value) => value.clone(),
        Value::StringUnits(value) => String::from_utf16_lossy(value),
        value => {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"alt\" argument must be of type string.{}",
                    crate::modules::util::invalid_arg_received(value)
                ),
            ));
        }
    };
    if !alt.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_CHAR",
            "Invalid character in alt".into(),
        ));
    }
    let origin_or_stream = args.get(1).unwrap_or(&Value::Undefined);
    let (stream_id, origin) = match origin_or_stream {
        Value::Undefined | Value::Null => (0_u32, String::new()),
        Value::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value > 0.0
                && *value <= u32::MAX as f64 =>
        {
            (*value as u32, String::new())
        }
        Value::Number(value) => {
            return Err(coded_error(
                quench_runtime::ops::Builtin::RangeError,
                "ERR_OUT_OF_RANGE",
                format!(
                    "The value of \"originOrStream\" is out of range. It must be > 0 && < 4294967296. Received {}",
                    display_number(*value)
                ),
            ));
        }
        Value::String(value) => (
            0,
            valid_alt_origin(value).ok_or_else(|| {
                coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_HTTP2_ALTSVC_INVALID_ORIGIN",
                    "HTTP/2 ALTSVC frames require a valid origin".into(),
                )
            })?,
        ),
        Value::StringUnits(value) => {
            let value = String::from_utf16_lossy(value);
            (
                0,
                valid_alt_origin(&value).ok_or_else(|| {
                    coded_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_HTTP2_ALTSVC_INVALID_ORIGIN",
                        "HTTP/2 ALTSVC frames require a valid origin".into(),
                    )
                })?,
            )
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            let value = execute::get_property(origin_or_stream, "origin");
            let Value::String(value) = value else {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_TYPE",
                    "The \"originOrStream\" argument must be of type string or number".into(),
                ));
            };
            (
                0,
                valid_alt_origin(&value).ok_or_else(|| {
                    coded_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_HTTP2_ALTSVC_INVALID_ORIGIN",
                        "HTTP/2 ALTSVC frames require a valid origin".into(),
                    )
                })?,
            )
        }
        value => {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"originOrStream\" argument must be of type string or number.{}",
                    crate::modules::util::invalid_arg_received(value)
                ),
            ));
        }
    };
    if origin.len() + alt.len() + 2 > 16_382 {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_HTTP2_ALTSVC_LENGTH",
            "HTTP/2 ALTSVC frames are limited to 16382 bytes".into(),
        ));
    }
    if stream_id != 0 {
        let exists = state
            .borrow()
            .net
            .http2_streams
            .contains_key(&(crate::modules::net::net_id(socket).unwrap_or(0), stream_id));
        if !exists {
            return Ok(Value::Undefined);
        }
    }
    let mut payload = (origin.len() as u16).to_be_bytes().to_vec();
    payload.extend_from_slice(origin.as_bytes());
    payload.extend_from_slice(alt.as_bytes());
    let frame = crate::modules::http2_protocol::Frame::new(
        crate::modules::http2_protocol::FrameType::AltSvc,
        0,
        stream_id,
        payload,
    );
    let write = execute::get_property(socket, "write");
    if quench_runtime::is_callable(&write) {
        execute::call(
            &write,
            socket,
            &[crate::modules::buffer_proto::make_buffer(&frame.encode())],
        )?;
    }
    Ok(Value::Undefined)
}

fn valid_alt_origin(value: &str) -> Option<String> {
    let (scheme, rest) = value.split_once("://")?;
    if !matches!(scheme, "http" | "https") || rest.is_empty() {
        return None;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.bytes().any(|byte| byte <= 0x20) {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

fn display_number(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "-Infinity".into()
        } else {
            "Infinity".into()
        }
    } else {
        value.to_string()
    }
}

fn session_method(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    values: &[Value],
) -> Result<Value, VmError> {
    let socket = receiver.ok_or(VmError::NotCallable)?;
    if matches!(
        execute::get_property(socket, "destroyed"),
        Value::Boolean(true)
    ) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::Error,
            "ERR_HTTP2_INVALID_SESSION",
            "The session has been destroyed".into(),
        ));
    }
    let method = match values.first() {
        Some(Value::String(name)) => name.as_str(),
        _ => return Err(VmError::NotCallable),
    };
    let args = &values[1..];
    match method {
        "altsvc" => return session_altsvc(state, socket, args),
        "setNextStreamID" => {
            let value = args.first().unwrap_or(&Value::Undefined);
            let Value::Number(id) = value else {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_TYPE",
                    format!(
                        "The \"id\" argument must be of type number.{}",
                        crate::modules::util::invalid_arg_received(value)
                    ),
                ));
            };
            if !id.is_finite() || id.fract() != 0.0 || *id < 1.0 || *id > u32::MAX as f64 {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::RangeError,
                    "ERR_OUT_OF_RANGE",
                    format!(
                        "The value of \"id\" is out of range. It must be > 0 and <= 4294967295. Received {}",
                        id
                    ),
                ));
            }
            execute::set_property_in_place(
                socket,
                "\0quench:http2-next-stream-id",
                Value::Number(*id),
            );
            let session_state = execute::get_property(socket, "state");
            execute::set_property_in_place(&session_state, "nextStreamID", Value::Number(*id));
            return Ok(Value::Undefined);
        }
        "setLocalWindowSize" => {
            let value = args.first().unwrap_or(&Value::Undefined);
            let Value::Number(window) = value else {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_TYPE",
                    format!(
                        "The \"windowSize\" argument must be of type number.{}",
                        crate::modules::util::invalid_arg_received(value)
                    ),
                ));
            };
            if !window.is_finite()
                || window.fract() != 0.0
                || *window < 0.0
                || *window > 2_147_483_647.0
            {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::RangeError,
                    "ERR_OUT_OF_RANGE",
                    format!(
                        "The value of \"windowSize\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
                        window
                    ),
                ));
            }
            let session_state = execute::get_property(socket, "state");
            execute::set_property_in_place(
                &session_state,
                "effectiveLocalWindowSize",
                Value::Number(*window),
            );
            return Ok(Value::Undefined);
        }
        "settings" => {
            let settings = args.first().unwrap_or(&Value::Undefined);
            if matches!(settings, Value::Undefined) {
                // `session.settings(undefined, callback)` is a valid
                // no-op shape used when callers only exercise destruction;
                // Node does not invoke the callback for this absent update.
                return Ok(Value::Undefined);
            }
            if !matches!(settings, Value::Object(_) | Value::ObjectAlias(_)) {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_TYPE",
                    format!(
                        "The \"settings\" argument must be of type object.{}",
                        crate::modules::util::invalid_arg_received(settings)
                    ),
                ));
            }
            // Reuse the canonical settings encoder for all range/type/custom
            // setting validation, then send the same bytes through the
            // transport and update the stable localSettings object.
            let payload = packed_settings_payload(settings).ok_or_else(|| {
                coded_error(
                    quench_runtime::ops::Builtin::Error,
                    "ERR_HTTP2_INVALID_SETTING_VALUE",
                    "Unable to encode HTTP/2 settings".into(),
                )
            })?;
            if let Some(callback) = args.get(1) {
                if !quench_runtime::is_callable(callback) {
                    return Err(coded_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_INVALID_ARG_TYPE",
                        format!(
                            "The \"callback\" argument must be of type function.{}",
                            crate::modules::util::invalid_arg_received(callback)
                        ),
                    ));
                }
            }
            let write = execute::get_property(socket, "write");
            if quench_runtime::is_callable(&write) {
                let frame = crate::modules::http2_protocol::Frame::new(
                    crate::modules::http2_protocol::FrameType::Settings,
                    0,
                    0,
                    payload,
                );
                execute::call(
                    &write,
                    socket,
                    &[crate::modules::buffer_proto::make_buffer(&frame.encode())],
                )?;
            }
            update_local_settings(socket, settings);
            let local = execute::get_property(socket, "localSettings");
            crate::modules::net::emit(state, socket, "localSettings", vec![local])?;
            execute::set_property_in_place(socket, "pendingSettingsAck", Value::Boolean(true));
            if let Some(callback) = args.get(1) {
                if quench_runtime::is_callable(callback) {
                    execute::call(callback, &Value::Undefined, &[])?;
                }
            }
            return Ok(Value::Undefined);
        }
        "ping" => {
            let (payload, callback) = match args.first() {
                Some(value) if quench_runtime::is_callable(value) => (vec![0; 8], value.clone()),
                Some(value) => {
                    let payload = ping_payload(value).ok_or_else(|| {
                        coded_error(
                            quench_runtime::ops::Builtin::TypeError,
                            "ERR_INVALID_ARG_TYPE",
                            format!(
                                "The \"payload\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
                                crate::modules::util::invalid_arg_received(value)
                            ),
                        )
                    })?;
                    (payload, args.get(1).cloned().unwrap_or(Value::Undefined))
                }
                None => (vec![0; 8], Value::Undefined),
            };
            if payload.len() != 8 {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::RangeError,
                    "ERR_HTTP2_PING_LENGTH",
                    "HTTP2 ping payload must be 8 bytes".into(),
                ));
            }
            if !matches!(callback, Value::Undefined) && !quench_runtime::is_callable(&callback) {
                return Err(coded_error(
                    quench_runtime::ops::Builtin::TypeError,
                    "ERR_INVALID_ARG_TYPE",
                    format!(
                        "The \"callback\" argument must be of type function.{}",
                        crate::modules::util::invalid_arg_received(&callback)
                    ),
                ));
            }
            let resource = crate::modules::async_hooks::new_resource(
                state,
                &[Value::String("HTTP2PING".into())],
            )?;
            let limit = match execute::get_property(socket, HTTP2_MAX_OUTSTANDING_PINGS) {
                Value::Number(limit) if limit.is_finite() && limit >= 0.0 => limit as usize,
                _ => 10,
            };
            let socket_id = crate::modules::net::net_id(socket).ok_or(VmError::NotCallable)?;
            let outstanding = state
                .borrow()
                .net
                .http2_pings
                .get(&socket_id)
                .map_or(0, Vec::len);
            let pending = crate::modules::net::PendingHttp2Ping {
                payload: payload.clone(),
                callback,
                resource,
                started: std::time::Instant::now(),
            };
            if outstanding >= limit {
                invoke_ping_callback(state, pending, Some(ping_error()))?;
                return Ok(Value::Boolean(false));
            }
            let frame = crate::modules::http2_protocol::Frame::new(
                crate::modules::http2_protocol::FrameType::Ping,
                0,
                0,
                payload,
            );
            let write = execute::get_property(socket, "write");
            if quench_runtime::is_callable(&write) {
                execute::call(
                    &write,
                    socket,
                    &[crate::modules::buffer_proto::make_buffer(&frame.encode())],
                )?;
            }
            state
                .borrow_mut()
                .net
                .http2_pings
                .entry(socket_id)
                .or_default()
                .push(pending);
            return Ok(Value::Boolean(true));
        }
        "goaway" => {
            let socket_id = crate::modules::net::net_id(socket).ok_or(VmError::NotCallable)?;
            let code = match args.first().unwrap_or(&Value::Number(0.0)) {
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && *value >= 0.0
                        && *value <= u32::MAX as f64 =>
                {
                    *value as u32
                }
                value => {
                    return Err(coded_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_INVALID_ARG_TYPE",
                        format!(
                            "The \"code\" argument must be of type number.{}",
                            crate::modules::util::invalid_arg_received(value)
                        ),
                    ));
                }
            };
            let requested_last = match args.get(1).unwrap_or(&Value::Number(0.0)) {
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && *value >= 0.0
                        && *value <= u32::MAX as f64 =>
                {
                    *value as u32
                }
                value => {
                    return Err(coded_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_INVALID_ARG_TYPE",
                        format!(
                            "The \"lastStreamID\" argument must be of type number.{}",
                            crate::modules::util::invalid_arg_received(value)
                        ),
                    ));
                }
            };
            let opaque = match args.get(2) {
                None | Some(Value::Undefined) => Vec::new(),
                Some(value) => ping_payload(value).ok_or_else(|| {
                    coded_error(
                        quench_runtime::ops::Builtin::TypeError,
                        "ERR_INVALID_ARG_TYPE",
                        format!(
                            "The \"opaqueData\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
                            crate::modules::util::invalid_arg_received(value)
                        ),
                    )
                })?,
            };
            let last_stream_id = if requested_last == 0 {
                state
                    .borrow()
                    .net
                    .http2_sessions
                    .get(&socket_id)
                    .and_then(|session| session.streams.keys().copied().max())
                    .unwrap_or(0)
            } else {
                requested_last
            };
            let mut payload = last_stream_id.to_be_bytes().to_vec();
            payload.extend_from_slice(&code.to_be_bytes());
            payload.extend_from_slice(&opaque);
            let frame = crate::modules::http2_protocol::Frame::new(
                crate::modules::http2_protocol::FrameType::GoAway,
                0,
                0,
                payload,
            );
            let write = execute::get_property(socket, "write");
            if quench_runtime::is_callable(&write) {
                execute::call(
                    &write,
                    socket,
                    &[crate::modules::buffer_proto::make_buffer(&frame.encode())],
                )?;
            }
            if let Some(callback) = args
                .get(3)
                .filter(|value| quench_runtime::is_callable(value))
            {
                execute::call(callback, socket, &[])?;
            }
        }
        _ => return Err(VmError::NotCallable),
    }
    Ok(Value::Undefined)
}

fn connect_target_options(
    authority: &Value,
    extra_options: Option<&Value>,
) -> Result<Value, VmError> {
    let mut target = match authority {
        Value::String(_) | Value::StringUnits(_) => parse_authority(authority)?,
        Value::Object(_) | Value::ObjectAlias(_) => {
            let mut target = host_api::object(Vec::new());
            for key in execute::own_enumerable_keys(authority) {
                let value = execute::get_property(authority, &key);
                execute::set_property_in_place(&target, &key, value);
            }
            // WHATWG URL instances keep their connection fields on the
            // prototype/internal slots rather than as enumerable own keys.
            // Read the ordinary URL properties as facts too, while retaining
            // enumerable option objects unchanged.
            for key in ["protocol", "hostname", "host", "port", "path", "pathname"] {
                if matches!(execute::get_property(&target, key), Value::Undefined) {
                    let value = execute::get_property(authority, key);
                    if !matches!(value, Value::Undefined) {
                        execute::set_property_in_place(&target, key, value);
                    }
                }
            }
            target
        }
        _ => {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                "The \"authority\" argument must be a string or an object.".into(),
            ));
        }
    };
    if let Some(options) = extra_options {
        for key in execute::own_enumerable_keys(options) {
            execute::set_property_in_place(&target, &key, execute::get_property(options, &key));
        }
    }
    if matches!(execute::get_property(&target, "host"), Value::Undefined) {
        let hostname = execute::get_property(&target, "hostname");
        if !matches!(hostname, Value::Undefined) {
            execute::set_property_in_place(&target, "host", hostname);
        }
    } else if let (Value::String(host), Value::String(hostname), Value::String(port)) = (
        execute::get_property(&target, "host"),
        execute::get_property(&target, "hostname"),
        execute::get_property(&target, "port"),
    ) {
        // URL.host includes the port, whereas net/tls expect host and port
        // as separate options. Keep explicit option objects untouched and
        // only split the canonical URL-derived form.
        if host == format!("{hostname}:{port}") || host == format!("[{hostname}]:{port}") {
            execute::set_property_in_place(
                &target,
                "host",
                Value::String(hostname.trim_matches(['[', ']']).into()),
            );
        } else if let Some(host) = host.strip_prefix('[') {
            if let Some(end) = host.find(']') {
                execute::set_property_in_place(&target, "host", Value::String(host[..end].into()));
            }
        }
    }
    if let Value::String(protocol) = execute::get_property(&target, "protocol") {
        if !protocol.ends_with(':') {
            execute::set_property_in_place(
                &target,
                "protocol",
                Value::String(format!("{protocol}:").into()),
            );
        }
    }
    // `http2.connect('https://…')` negotiates HTTP/2 via ALPN even when the
    // caller does not spell out TLS options.  Supplying the protocol token at
    // this boundary keeps the TLS transport from rejecting an otherwise valid
    // secure session before the ClientHttp2Session exists.
    let secure = matches!(
        execute::get_property(&target, "protocol"),
        Value::String(protocol) if protocol.eq_ignore_ascii_case("https:")
    );
    if secure
        && matches!(
            execute::get_property(&target, "ALPNProtocols"),
            Value::Undefined | Value::Null
        )
    {
        execute::set_property_in_place(
            &target,
            "ALPNProtocols",
            host_api::array(vec![Value::String("h2".into())]),
        );
    }
    Ok(target)
}

fn parse_authority(authority: &Value) -> Result<Value, VmError> {
    let text = execute::to_js_string(authority)?;
    let Some((scheme, remainder)) = text.split_once("://") else {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_URL",
            format!("Invalid URL: {text}"),
        ));
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(coded_error(
            quench_runtime::ops::Builtin::Error,
            "ERR_HTTP2_UNSUPPORTED_PROTOCOL",
            format!("Protocol \"{scheme}:\" not supported."),
        ));
    }
    let authority = remainder.split('/').next().unwrap_or_default();
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_URL",
                format!("Invalid URL: {text}"),
            ));
        };
        let host = &rest[..end];
        let port = rest[end + 1..].strip_prefix(':');
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && !port.is_empty() => (host, Some(port)),
            _ => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_URL",
            format!("Invalid URL: {text}"),
        ));
    }
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        "443"
    } else {
        "80"
    };
    Ok(host_api::object(vec![
        (
            "protocol".into(),
            Value::String(format!("{scheme}:").into()),
        ),
        (
            "host".into(),
            Value::String(host.trim_matches(['[', ']']).into()),
        ),
        (
            "port".into(),
            Value::String(port.unwrap_or(default_port).into()),
        ),
    ]))
}

/// Validate the option boundary shared by the HTTP/2 server constructors.
/// The protocol transport is intentionally unavailable, but rejecting bad
/// arguments before reporting that capability boundary preserves the public
/// API's ordinary error contract without fabricating a server.
fn create_server(
    state: &Rc<RefCell<HostState>>,
    values: &[Value],
    secure: bool,
) -> Result<Value, VmError> {
    let options = values.first().unwrap_or(&Value::Undefined);
    if !matches!(
        options,
        Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
    ) && !quench_runtime::is_callable(options)
    {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_INVALID_ARG_TYPE",
            format!(
                "The \"options\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(options)
            ),
        ));
    }
    if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        let alpn_callback = execute::get_property(options, "ALPNCallback");
        let alpn_protocols = execute::get_property(options, "ALPNProtocols");
        // Node's TLS server treats these as mutually exclusive: a callback
        // owns protocol selection, so a static protocol list would make the
        // negotiation fact ambiguous.  HTTP/2 delegates its secure server
        // transport to the same TLS boundary and must preserve that error
        // before creating a listener.
        if secure
            && !matches!(alpn_callback, Value::Undefined | Value::Null)
            && !matches!(alpn_protocols, Value::Undefined | Value::Null)
        {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS",
                "The ALPNCallback and ALPNProtocols options are mutually exclusive".into(),
            ));
        }
        let settings = execute::get_property(options, "settings");
        if !matches!(
            settings,
            Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_INVALID_ARG_TYPE",
                format!(
                    "The \"options.settings\" property must be of type object.{}",
                    crate::modules::util::invalid_arg_received(&settings)
                ),
            ));
        }
        for name in ["maxSessionInvalidFrames", "maxSessionRejectedStreams"] {
            let value = execute::get_property(options, name);
            if let Value::Number(value) = value {
                if value.is_nan() || value.is_sign_negative() {
                    return Err(coded_error(
                        quench_runtime::ops::Builtin::RangeError,
                        "ERR_OUT_OF_RANGE",
                        format!("The value of \"options.{name}\" is out of range."),
                    ));
                }
            }
        }
    }
    // Reuse the canonical Rust-owned listener lifecycle.  This exposes the
    // valid constructor/listen/address/close surface without claiming that a
    // connected socket has HTTP/2 session or stream semantics; those remain a
    // separate protocol capability layered above this endpoint identity.
    // The constructor's callback is an HTTP/2 request handler, not a raw
    // net.Server `connection` listener.  Do not register it on the transport
    // endpoint (which would call `mustNotCall` handlers as soon as a TCP peer
    // connects); retain it for the future session layer instead.
    let transport_values = if quench_runtime::is_callable(options) {
        &[][..]
    } else {
        &values[..values.len().min(1)]
    };
    let request_listener = if quench_runtime::is_callable(options) {
        Some(options.clone())
    } else {
        values
            .get(1)
            .filter(|value| quench_runtime::is_callable(value))
            .cloned()
    };
    let server = if secure {
        crate::modules::tls::create_server(state, None, transport_values)?
    } else {
        crate::modules::net::create_server(state, transport_values)?
    };
    if let Some(request_listener) = request_listener {
        crate::modules::net::register_http2_request_listener(
            state,
            &server,
            request_listener.clone(),
        );
        execute::set_property_in_place(
            &server,
            "\0quench:http2-request-listener",
            request_listener,
        );
    }
    if let Some(options) = values
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let settings = execute::get_property(options, "settings");
        if matches!(settings, Value::Object(_) | Value::ObjectAlias(_)) {
            execute::set_property_in_place(&server, "\0quench:http2-settings", settings.clone());
            crate::modules::net::register_http2_server_settings(state, &server, settings);
        }
        let remote_custom = execute::get_property(options, "remoteCustomSettings");
        if matches!(remote_custom, Value::Array(_)) {
            crate::modules::net::register_http2_server_remote_custom(state, &server, remote_custom);
        }
    }
    execute::set_property_in_place(
        &server,
        crate::modules::http2_protocol::SERVER_MARKER,
        Value::Boolean(true),
    );
    crate::modules::net::register_http2_server(state, &server);
    Ok(server)
}

fn session_name(values: &[Value]) -> Result<Value, VmError> {
    let name = match values.first() {
        Some(Value::Number(value)) if *value == 0.0 => "server",
        Some(Value::Number(value)) if *value == 1.0 => "client",
        _ => "<invalid>",
    };
    Ok(Value::String(name.into()))
}

fn get_authority(values: &[Value]) -> Result<Value, VmError> {
    let headers = values.first().unwrap_or(&Value::Undefined);
    let authority = execute::get_property(headers, ":authority");
    let value = if !matches!(authority, Value::Undefined) {
        authority
    } else {
        execute::get_property(headers, "host")
    };
    Ok(value)
}

fn build_ng_header_string(values: &[Value]) -> Result<Value, VmError> {
    let headers = values.first().unwrap_or(&Value::Undefined);
    let validator = values.get(1).unwrap_or(&Value::Undefined);
    let strict = matches!(values.get(2), Some(Value::Boolean(true)));
    let sensitive = sensitive_names(headers)?;
    let mut fields = Vec::new();
    let mut seen_pseudo = HashSet::new();
    let keys = execute::own_enumerable_keys(headers);
    let ordered_keys = keys
        .iter()
        .filter(|key| key.starts_with(':'))
        .chain(keys.iter().filter(|key| !key.starts_with(':')));
    for key in ordered_keys {
        if key.is_empty() || key.contains('\0') {
            continue;
        }
        let lower = key.to_ascii_lowercase();
        validate_header_name(&lower)?;
        if lower.starts_with(':') && !seen_pseudo.insert(lower.clone()) {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_HTTP2_HEADER_SINGLE_VALUE",
                format!("Header field \"{lower}\" must only have a single value"),
            ));
        }
        if lower.starts_with(':') && quench_runtime::is_callable(validator) {
            execute::call(
                validator,
                &Value::Undefined,
                &[Value::String(lower.clone())],
            )?;
        }
        let raw = execute::get_property(headers, key);
        let values = header_values(&raw);
        if lower == "te"
            && values
                .iter()
                .any(|value| !value.eq_ignore_ascii_case("trailers"))
        {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_HTTP2_INVALID_CONNECTION_HEADERS",
                "HTTP/1 Connection specific headers are forbidden: \"te\"".into(),
            ));
        }
        if strict && SINGLE_VALUE_HEADERS.contains(&lower.as_str()) && values.len() > 1 {
            return Err(coded_error(
                quench_runtime::ops::Builtin::TypeError,
                "ERR_HTTP2_HEADER_SINGLE_VALUE",
                format!("Header field \"{lower}\" must only have a single value"),
            ));
        }
        fields.extend(values.into_iter().map(|value| (lower.clone(), value)));
    }
    let mut encoded = String::new();
    let mut previous_sensitive = false;
    for (index, (name, value)) in fields.iter().enumerate() {
        if index > 0 && !previous_sensitive {
            encoded.push('\0');
        }
        encoded.push_str(name);
        encoded.push('\0');
        encoded.push_str(value);
        encoded.push('\0');
        previous_sensitive = sensitive.contains(name);
        if previous_sensitive {
            encoded.push('\x01');
        }
    }
    if !previous_sensitive {
        encoded.push('\0');
    }
    Ok(host_api::array(vec![
        Value::String(encoded),
        Value::Number(fields.len() as f64),
    ]))
}

fn header_values(value: &Value) -> Vec<String> {
    match value {
        Value::Array(_) => execute::own_enumerable_keys(value)
            .into_iter()
            .filter_map(|key| execute::to_js_string(&execute::get_property(value, &key)).ok())
            .collect(),
        _ => execute::to_js_string(value).ok().into_iter().collect(),
    }
}

fn sensitive_names(headers: &Value) -> Result<HashSet<String>, VmError> {
    let mut names = HashSet::new();
    for symbol in execute::own_enumerable_symbol_strings(headers) {
        if !symbol.starts_with("Symbol.nodejs.http2.sensitiveHeaders") {
            continue;
        }
        for value in header_values(&execute::get_property(headers, &symbol)) {
            names.insert(value.to_ascii_lowercase());
        }
    }
    Ok(names)
}

fn validate_header_name(name: &str) -> Result<(), VmError> {
    if CONNECTION_HEADERS.contains(&name) {
        return Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_HTTP2_INVALID_CONNECTION_HEADERS",
            format!("HTTP/1 Connection specific headers are forbidden: \"{name}\""),
        ));
    }
    Ok(())
}

fn to_header_object(values: &[Value]) -> Result<Value, VmError> {
    let raw = values.first().unwrap_or(&Value::Undefined);
    let mut result = host_api::object(Vec::new());
    let mut index = 0;
    while index + 1 < execute::own_enumerable_keys(raw).len() {
        let key = execute::get_property(raw, &index.to_string());
        let value = execute::get_property(raw, &(index + 1).to_string());
        let key = execute::to_js_string(&key)?;
        let value = execute::to_js_string(&value)?;
        result = merge_header_value(result, &key, value)?;
        index += 2;
    }
    Ok(result)
}

fn merge_header_value(mut result: Value, key: &str, value: String) -> Result<Value, VmError> {
    let old = execute::get_property(&result, key);
    let old_set_cookies = (key == "set-cookie").then(|| {
        execute::own_enumerable_keys(&old)
            .into_iter()
            .map(|key| execute::get_property(&old, &key))
            .collect::<Vec<_>>()
    });
    let merged = match (key, old) {
        ("set-cookie", Value::Undefined) => host_api::array(vec![Value::String(value)]),
        ("set-cookie", Value::Array(_)) => {
            let mut items = old_set_cookies.unwrap_or_default();
            items.push(Value::String(value));
            host_api::array(items)
        }
        (_, Value::Undefined) if key == ":status" => {
            Value::Number(value.parse::<f64>().unwrap_or(f64::NAN))
        }
        (_, current)
            if !matches!(current, Value::Undefined)
                && SINGLE_VALUE_HEADERS.contains(&key.to_ascii_lowercase().as_str()) =>
        {
            current
        }
        (_, Value::Undefined) => Value::String(value),
        (_, Value::String(old)) if key == "cookie" => Value::String(format!("{old}; {value}")),
        (_, Value::String(old)) => Value::String(format!("{old}, {value}")),
        (_, current) => current,
    };
    Ok(execute::set_property(result, key, merged))
}

fn update_options_buffer(values: &[Value]) -> Result<Value, VmError> {
    let options = values
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .unwrap_or(&Value::Undefined);
    let buffer = options_buffer();
    for (name, index) in OPTION_FIELDS {
        if execute::has_own_property(options, name) {
            let value = quench_runtime::to_number(&execute::get_property(options, name))?;
            execute::set_array_index_in_place(&buffer, *index, Value::Number(value));
        }
    }
    if execute::has_own_property(options, "strictFieldWhitespaceValidation") {
        let value = execute::get_property(options, "strictFieldWhitespaceValidation");
        let strict = matches!(value, Value::Boolean(false));
        execute::set_array_index_in_place(&buffer, 12, Value::Number(strict as u8 as f64));
    }
    let flags = OPTION_FIELDS.iter().fold(0u32, |flags, (_, index)| {
        let value = execute::get_property(&buffer, &index.to_string());
        if matches!(value, Value::Number(value) if value != 0.0) {
            flags | (1 << index)
        } else {
            flags & !(1 << index)
        }
    });
    let flags = if matches!(execute::get_property(&buffer, "12"), Value::Number(value) if value != 0.0)
    {
        flags | (1 << 12)
    } else {
        flags & !(1 << 12)
    };
    execute::set_array_index_in_place(&buffer, 13, Value::Number(flags as f64));
    Ok(Value::Undefined)
}

pub fn construct_nghttp_error(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Constructors created with `make(kind)` carry the dispatch tag as a
    // bound argument; only the user-supplied errno belongs to the error.
    nghttp_error(args.get(1..).unwrap_or_default())
}

fn nghttp_error(values: &[Value]) -> Result<Value, VmError> {
    let errno = match values.first() {
        Some(Value::Number(value)) => *value as i64,
        _ => 0,
    };
    let message = nghttp_error_message(errno);
    let mut error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    execute::set_property_in_place(&error, "code", Value::String("ERR_HTTP2_ERROR".into()));
    execute::set_property_in_place(&error, "errno", Value::Number(errno as f64));
    let prototype = execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "\0quench:http2-nghttp-prototype",
    );
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        error = execute::set_prototype_of(&error, &prototype)?;
    }
    let constructor = execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "\0quench:http2-nghttp-constructor",
    );
    if matches!(constructor, Value::Function(_) | Value::BoundFunction(_)) {
        error = execute::define_property(
            error,
            "constructor",
            host_api::object(vec![
                ("value".into(), constructor),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(false)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )?;
    }
    Ok(error)
}

fn nghttp_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(VmError::NotCallable);
    };
    let code = execute::get_property(receiver, "code");
    let message = execute::get_property(receiver, "message");
    let code = execute::to_js_string(&code).unwrap_or_default();
    let message = execute::to_js_string(&message).unwrap_or_default();
    Ok(Value::String(format!("Error [{code}]: {message}")))
}

fn nghttp_error_string(values: &[Value]) -> Result<Value, VmError> {
    let errno = match values.first() {
        Some(Value::Number(value)) => *value as i32,
        _ => 0,
    };
    let message = nghttp_error_message(i64::from(errno));
    Ok(Value::String(message.into()))
}

fn nghttp_error_message(errno: i64) -> &'static str {
    match errno {
        -501 => "Invalid argument",
        -508 => "Operation would block",
        -509 => "Stream ID not available",
        -510 => "Stream closed",
        -523 => "Protocol error",
        -517 => "GOAWAY has already been sent",
        -522 => "Frame size error",
        -901 => "Out of memory",
        _ => "Unknown error code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(value: &Value) -> Vec<u8> {
        let Value::Uint8Array(view) = value else {
            panic!("expected byte array");
        };
        view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
    }

    #[test]
    fn default_settings_have_stable_wire_order() {
        let packed = packed_settings(&[default_settings()]).expect("pack defaults");
        assert_eq!(
            bytes(&packed),
            vec![
                0, 1, 0, 0, 16, 0, 0, 2, 0, 0, 0, 1, 0, 3, 255, 255, 255, 255, 0, 4, 0, 64, 0, 0,
                0, 5, 0, 0, 64, 0, 0, 6, 0, 0, 255, 255, 0, 8, 0, 0, 0, 0,
            ]
        );
    }

    #[test]
    fn client_preface_includes_initial_settings_frame() {
        let bytes = client_preface();
        assert_eq!(&bytes[..24], b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        assert_eq!(&bytes[24..], &[0, 0, 0, 4, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn settings_round_trip_custom_values() {
        let settings = host_api::object(vec![
            ("headerTableSize".into(), Value::Number(100.0)),
            ("maxFrameSize".into(), Value::Number(20_000.0)),
            (
                "customSettings".into(),
                host_api::object(vec![("9999".into(), Value::Number(301.0))]),
            ),
        ]);
        let packed = packed_settings(&[settings]).expect("pack settings");
        let decoded = unpacked_settings(&[packed]).expect("unpack settings");
        assert_eq!(
            execute::get_property(&decoded, "headerTableSize"),
            Value::Number(100.0)
        );
        assert_eq!(
            execute::get_property(&decoded, "maxFrameSize"),
            Value::Number(20_000.0)
        );
        let custom = execute::get_property(&decoded, "customSettings");
        assert_eq!(execute::get_property(&custom, "9999"), Value::Number(301.0));
    }
}
