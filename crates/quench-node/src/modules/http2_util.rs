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

pub fn binding() -> Value {
    host_api::object(vec![
        ("constants".into(), header_constants()),
        ("optionsBuffer".into(), options_buffer()),
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
        _ => Err(VmError::NotCallable),
    }
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
