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
}
use super::http2_asserts;
use super::http2_facts::{
    CONNECTION_HEADERS, HEADER_CONSTANTS, OPTION_FIELDS, SINGLE_VALUE_HEADERS,
};

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
    ]);
    let global = quench_runtime::vm::current_global_object();
    execute::set_property_in_place(&global, "__quenchHttp2Binding", binding());
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
    push_numeric_setting(settings, "headerTableSize", 1, 0.0, u32::MAX as f64, &mut entries)?;
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
                quench_runtime::ops::Builtin::RangeError,
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
    let mut result = host_api::object(Vec::new());
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
                    return invalid_setting("initialWindowSize", &Value::Number(value as f64), false);
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
            .filter_map(|index| match quench_runtime::to_number(&execute::get_property(
                value,
                &index.to_string(),
            )) {
                Ok(number) if number.is_finite() => Some(number as u8),
                _ => None,
            })
            .collect(),
    )
}

pub fn binding() -> Value {
    let session = host_api::bound_builtin(
        quench_runtime::ops::Builtin::Object,
        Value::Undefined,
    );
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

fn header_constants() -> Value {
    host_api::object(
        HEADER_CONSTANTS
            .iter()
            .map(|(name, value)| ((*name).into(), Value::String((*value).into())))
            .collect(),
    )
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
    _state: &Rc<RefCell<HostState>>,
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
        _ => Err(VmError::NotCallable),
    }
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
    let message = match errno {
        -501 => "Invalid argument",
        _ => "Unknown error code",
    };
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
    let message = match errno {
        -501 => "Invalid argument",
        -508 => "Operation would block",
        -509 => "Stream ID not available",
        -510 => "Stream closed",
        -517 => "GOAWAY has already been sent",
        -522 => "Frame size error",
        -901 => "Out of memory",
        _ => "Unknown error code",
    };
    Ok(Value::String(message.into()))
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
                0, 1, 0, 0, 16, 0, 0, 2, 0, 0, 0, 1, 0, 3, 255, 255, 255, 255, 0, 4,
                0, 64, 0, 0, 0, 5, 0, 0, 64, 0, 0, 6, 0, 0, 255, 255, 0, 8, 0, 0, 0, 0,
            ]
        );
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
