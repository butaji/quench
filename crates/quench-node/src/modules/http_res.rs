//! Server-side HTTP response (`res`) methods, split out of `http` to
//! keep both files under the line limit. State lives in the `res` map
//! on `http::HttpState`, keyed by a hidden `res` object property.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::http::{chunk_bytes, Res, RES_ID_PROP, RESPONSE_CLOSE_PENDING_PROP};
use crate::modules::net;

fn res_state(receiver: Option<&Value>) -> Option<u64> {
    let receiver = receiver?;
    match quench_runtime::vm::get_property(receiver, RES_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as u64),
        _ => None,
    }
}

fn body_chunk(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    if matches!(value, Some(Value::Array(_))) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"chunk\" argument must be of type string or an instance of Buffer or Uint8Array".into(),
        ));
    }
    Ok(super::http::chunk_bytes(value))
}

/// `res.setHeader(name, value)` — replace any existing header of that name.
pub fn res_set_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let headers_sent = state
        .borrow()
        .http
        .res
        .get(&id)
        .is_some_and(|res| res.headers_sent)
        || matches!(
            execute::get_property(receiver.unwrap_or(&Value::Undefined), "headersSent"),
            Value::Boolean(true)
        );
    if headers_sent {
        return Err(headers_sent_error(
            "Cannot set headers after they are sent to the client",
        ));
    }
    let name = args.first().map(execute::to_js_string).transpose()?;
    let Some(name) = name else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let values = header_values_for(&name, args.get(1))?;
    if values.is_empty() {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    for value in &values {
        validate_header_value(&name, value)?;
    }
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.headers
            .retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
        res.headers
            .extend(values.into_iter().map(|value| (name.clone(), value)));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn res_set_headers(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let headers_sent = state
        .borrow()
        .http
        .res
        .get(&id)
        .is_some_and(|res| res.headers_sent)
        || matches!(
            execute::get_property(receiver.unwrap_or(&Value::Undefined), "headersSent"),
            Value::Boolean(true)
        );
    if headers_sent {
        return Err(headers_sent_error(
            "Cannot set headers after they are sent to the client",
        ));
    }
    let Some(source) = args.first() else {
        return Err(invalid_headers_argument());
    };
    let entries = header_entries(source).ok_or_else(invalid_headers_argument)?;
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.headers.clear();
        for (name, value) in entries {
            validate_header_value(&name, &value)?;
            res.headers.push((name, value));
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn invalid_headers_argument() -> VmError {
    crate::modules::buffer_enc::invalid_arg_type(
        "The \"headers\" argument must be an instance of Headers or Map".into(),
    )
}

fn header_entries(source: &Value) -> Option<Vec<(String, String)>> {
    let is_headers = matches!(execute::get_property(source, "_entries"), Value::Array(_));
    if !matches!(source, Value::Map(_)) && !is_headers {
        return None;
    }
    let entries = execute::get_property(source, "entries");
    if !quench_runtime::is_callable(&entries) {
        return None;
    }
    let iterator = execute::call(&entries, source, &[]).ok()?;
    let next = execute::get_property(&iterator, "next");
    if !quench_runtime::is_callable(&next) {
        return None;
    }
    let mut result = Vec::new();
    for _ in 0..10_000 {
        let step = execute::call(&next, &iterator, &[]).ok()?;
        if matches!(execute::get_property(&step, "done"), Value::Boolean(true)) {
            return Some(result);
        }
        let pair = execute::get_property(&step, "value");
        let Value::Array(_) = pair else {
            return None;
        };
        let name = execute::to_js_string(&execute::get_property(&pair, "0")).ok()?;
        let values = header_values(Some(&execute::get_property(&pair, "1"))).ok()?;
        for value in values {
            result.push((name.clone(), value));
        }
    }
    None
}

fn header_values(value: Option<&Value>) -> Result<Vec<String>, VmError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if let Value::Array(_) = value {
        let length = match execute::get_property(value, "length") {
            Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
            _ => 0,
        };
        return (0..length)
            .map(|index| execute::to_js_string(&execute::get_property(value, &index.to_string())))
            .collect();
    }
    Ok(vec![execute::to_js_string(value)?])
}

fn header_values_for(name: &str, value: Option<&Value>) -> Result<Vec<String>, VmError> {
    let mut values = header_values(value)?;
    if NON_REPEATABLE_HEADERS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        values.truncate(1);
    }
    Ok(values)
}

pub(crate) const NON_REPEATABLE_HEADERS: &[&str] = &[
    "content-type",
    "user-agent",
    "referer",
    "host",
    "authorization",
    "proxy-authorization",
    "if-modified-since",
    "if-unmodified-since",
    "from",
    "location",
    "max-forwards",
    "retry-after",
    "etag",
    "last-modified",
    "server",
    "age",
    "expires",
];

pub fn res_remove_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let headers_sent = state
        .borrow()
        .http
        .res
        .get(&id)
        .is_some_and(|res| res.headers_sent)
        || matches!(
            execute::get_property(receiver.unwrap_or(&Value::Undefined), "headersSent"),
            Value::Boolean(true)
        );
    if headers_sent {
        return Err(headers_sent_error(
            "Cannot remove headers after they are sent to the client",
        ));
    }
    let Some(name) = args.first().map(execute::to_js_string).transpose()? else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    if let Some(res) = state.borrow_mut().http.res.get_mut(&id) {
        res.headers
            .retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `res.writeHead(statusCode[, reasonPhrase][, headers])`.
pub fn res_write_head(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(Value::Undefined);
    };
    let requested = args.first().unwrap_or(&Value::Undefined);
    let Some(status) = valid_status(requested) else {
        return Err(invalid_status(requested));
    };
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, "statusCode", Value::Number(status as f64));
        execute::set_property_in_place(receiver, "headersSent", Value::Boolean(true));
    }
    if let Some(Value::Array(_)) = args.get(1) {
        let array = args.get(1).cloned().unwrap_or(Value::Undefined);
        let mut guard = state.borrow_mut();
        if let Some(res) = guard.http.res.get_mut(&id) {
            res.status = status;
            let keys = execute::own_enumerable_keys(&array);
            for pair in keys.chunks(2) {
                let (Some(name), Some(value)) = (pair.first(), pair.get(1)) else {
                    continue;
                };
                let Ok(name) = execute::to_js_string(&execute::get_property(&array, name)) else {
                    continue;
                };
                let Ok(values) =
                    header_values_for(&name, Some(&execute::get_property(&array, value)))
                else {
                    continue;
                };
                for value in &values {
                    validate_header_value(&name, value)?;
                }
                res.headers
                    .retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
                res.headers
                    .extend(values.into_iter().map(|value| (name.clone(), value)));
            }
        }
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    if let Some(Value::Object(_)) = args.get(1) {
        let object = args.get(1).cloned().unwrap_or(Value::Undefined);
        let mut guard = state.borrow_mut();
        if let Some(res) = guard.http.res.get_mut(&id) {
            res.status = status;
            merge_headers(res, &object)?;
        }
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let mut text = String::new();
    if let Some(Value::String(s)) = args.get(1) {
        text = s.clone();
    }
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.status = status;
        if !text.is_empty() {
            res.text = text;
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn headers_sent_error(message: &str) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_HTTP_HEADERS_SENT".into()),
    ))
}

pub fn res_cork(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let depth = match execute::get_property(receiver, "writableCorked") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as u64,
        _ => 0,
    } + 1;
    execute::set_property_in_place(receiver, "writableCorked", Value::Number(depth as f64));
    if res_state(Some(receiver)).is_some() {
        let socket = execute::get_property(receiver, "socket");
        if matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
            let current = match execute::get_property(&socket, "writableCorked") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => value as u64,
                _ => 0,
            } + 1;
            net::set_socket_property(&socket, "writableCorked", Value::Number(current as f64));
        }
    }
    Ok(receiver.clone())
}

pub fn res_uncork(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let depth = match execute::get_property(receiver, "writableCorked") {
        Value::Number(value) if value.is_finite() && value > 0.0 => value as u64 - 1,
        _ => 0,
    };
    execute::set_property_in_place(receiver, "writableCorked", Value::Number(depth as f64));
    let socket = res_state(Some(receiver)).map(|_| execute::get_property(receiver, "socket"));
    if let Some(socket) = socket {
        let socket_depth = match execute::get_property(&socket, "writableCorked") {
            Value::Number(value) if value.is_finite() && value > 0.0 => value as u64 - 1,
            _ => 0,
        };
        net::set_socket_property(
            &socket,
            "writableCorked",
            Value::Number(socket_depth as f64),
        );
    }
    if depth == 0
        && matches!(
            execute::get_property(receiver, "writableNeedDrain"),
            Value::Boolean(true)
        )
    {
        execute::set_property_in_place(receiver, "writableNeedDrain", Value::Boolean(false));
        execute::set_property_in_place(receiver, "writableLength", Value::Number(0.0));
        net::emit(state, receiver, "drain", Vec::new())?;
    }
    Ok(receiver.clone())
}

fn merge_headers(res: &mut Res, object: &Value) -> Result<(), VmError> {
    for key in execute::own_enumerable_keys(object) {
        if let Ok(item) = execute::get_property_result(object, &key) {
            if let Ok(values) = header_values_for(&key, Some(&item)) {
                for value in &values {
                    validate_header_value(&key, value)?;
                }
                res.headers
                    .retain(|(name, _)| !name.eq_ignore_ascii_case(&key));
                res.headers
                    .extend(values.into_iter().map(|value| (key.clone(), value)));
            }
        }
    }
    Ok(())
}

fn validate_header_value(name: &str, value: &str) -> Result<(), VmError> {
    if value
        .chars()
        .any(|character| character != '\t' && (character as u32) < 0x20 || character as u32 > 0xFF)
    {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(format!(
                "Invalid character in header content [\"{name}\"]"
            ))],
        );
        return Err(VmError::Thrown(execute::set_property(
            error,
            "code",
            Value::String("ERR_INVALID_CHAR".into()),
        )));
    }
    Ok(())
}

/// `res.write(chunk)` — buffer a body fragment.
pub fn res_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(Value::Undefined);
    };
    let bytes = if matches!(args.first(), Some(Value::Undefined)) {
        Vec::new()
    } else {
        let value = args
            .first()
            .ok_or_else(|| execute::type_error("chunk required"))?;
        body_chunk(Some(value))?
    };
    let high_water_mark = state
        .borrow()
        .http
        .res
        .get(&id)
        .map(|res| {
            execute::get_property(
                &execute::get_property(&res.socket, "_writableState"),
                "highWaterMark",
            )
        })
        .and_then(|value| match value {
            Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value),
            _ => None,
        })
        .unwrap_or(16_384.0);
    let current_length = match receiver.map(|value| execute::get_property(value, "writableLength"))
    {
        Some(Value::Number(value)) if value.is_finite() && value >= 0.0 => value,
        _ => 0.0,
    };
    // For an implicit chunked response, the writable queue counts the first
    // chunk's framing bytes as well as its payload. Empty writes enqueue no
    // chunk and therefore leave the length unchanged.
    let framing = if current_length == 0.0 && !bytes.is_empty() {
        5.0
    } else {
        0.0
    };
    let writable_length = current_length + bytes.len() as f64 + framing;
    let writable_ok = writable_length <= high_water_mark;
    if let Some(receiver) = receiver {
        execute::set_property_in_place(
            receiver,
            "writableHighWaterMark",
            Value::Number(high_water_mark),
        );
        execute::set_property_in_place(receiver, "writableLength", Value::Number(writable_length));
        execute::set_property_in_place(receiver, "writableNeedDrain", Value::Boolean(!writable_ok));
    }
    let (status, text, mut headers, socket, keep_alive, http10, send_date, first_write, chunked) = {
        let mut guard = state.borrow_mut();
        let response_socket = guard
            .http
            .res
            .get(&id)
            .map(|res| res.socket.clone())
            .unwrap_or(Value::Undefined);
        let connection_http10 = guard.http.conns.values().any(|conn| {
            execute::same_identity(&conn.socket, &response_socket)
                && matches!(
                    conn.req.as_ref().map(|req| execute::get_property(req, "httpVersion")),
                    Some(Value::String(version)) if version == "1.0"
                )
        });
        let Some(res) = guard.http.res.get_mut(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        res.body.extend_from_slice(&bytes);
        let first_write = !res.headers_sent;
        let http10 = res.http10 || connection_http10;
        if first_write
            && !http10
            && !res.headers.iter().any(|(key, _)| {
                key.eq_ignore_ascii_case("content-length")
                    || key.eq_ignore_ascii_case("transfer-encoding")
            })
        {
            res.headers
                .push(("Transfer-Encoding".into(), "chunked".into()));
        }
        let chunked = res.headers.iter().any(|(key, value)| {
            key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
        });
        res.chunked = chunked;
        res.headers_sent = true;
        res.sent_body = res.body.len();
        (
            res.status,
            res.text.clone(),
            res.headers.clone(),
            res.socket.clone(),
            res.keep_alive,
            res.http10,
            !matches!(
                execute::get_property(receiver.unwrap_or(&Value::Undefined), "sendDate"),
                Value::Boolean(false)
            ),
            first_write,
            chunked,
        )
    };
    let payload = if first_write {
        compose(
            status,
            &text,
            &headers,
            &bytes,
            response_keep_alive(&headers, keep_alive),
            http10,
            send_date,
        )
    } else {
        if chunked {
            chunk_frame(&bytes)
        } else {
            bytes
        }
    };
    if !payload.is_empty() {
        net::socket_write(state, Some(&socket), &[host_api::bytes(&payload)])?;
    }
    if let Some(callback) = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| {
            args.get(2)
                .filter(|value| quench_runtime::is_callable(value))
        })
    {
        execute::call(callback, receiver.unwrap_or(&Value::Undefined), &[])?;
    }
    Ok(Value::Boolean(writable_ok))
}

/// `res.writeContinue([callback])` — send the interim 100 response.
pub fn res_write_continue(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let socket = state
        .borrow()
        .http
        .res
        .get(&id)
        .map(|res| res.socket.clone());
    if let Some(socket) = socket {
        net::socket_write(
            state,
            Some(&socket),
            &[Value::String("HTTP/1.1 100 Continue\r\n\r\n".into())],
        )?;
    }
    if let Some(callback) = args.iter().find(|value| quench_runtime::is_callable(value)) {
        execute::call(callback, receiver.unwrap_or(&Value::Undefined), &[])?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `res.writeInformation(statusCode[, headers])` — send an interim 1xx
/// response without committing the final response headers.
pub fn res_write_information(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let Some(status) = args.first().and_then(valid_status) else {
        return Err(invalid_status(args.first().unwrap_or(&Value::Undefined)));
    };
    if !(100..200).contains(&status) || status == 101 {
        return Err(invalid_status(args.first().unwrap_or(&Value::Undefined)));
    }
    let mut headers = Vec::new();
    if let Some(source) = args.get(1) {
        for key in execute::own_enumerable_keys(source) {
            let name = key.to_string();
            let values = header_values_for(&name, Some(&execute::get_property(source, &key)))?;
            for value in values {
                validate_header_value(&name, &value)?;
                headers.push((name.clone(), value));
            }
        }
    }
    let socket = state
        .borrow()
        .http
        .res
        .get(&id)
        .map(|res| res.socket.clone());
    if let Some(socket) = socket {
        let mut payload = format!("HTTP/1.1 {status} {}\r\n", information_reason(status)).into_bytes();
        for (name, value) in headers {
            payload.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        payload.extend_from_slice(b"\r\n");
        net::socket_write(state, Some(&socket), &[host_api::bytes(&payload)])?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// Legacy spelling for a 102 Processing informational response.
pub fn res_write_processing(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let headers = args.first().cloned().unwrap_or(Value::Undefined);
    res_write_information(
        state,
        receiver,
        &[Value::Number(102.0), headers],
    )
}

fn information_reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        102 => "Processing",
        103 => "Early Hints",
        _ => "",
    }
}

/// `res.end([chunk])` — compose and send the response, then close the
/// socket.
pub fn res_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    if let Some(data) = args
        .first()
        .filter(|data| !matches!(data, Value::Undefined))
    {
        let headers_sent = state
            .borrow()
            .http
            .res
            .get(&id)
            .map(|res| res.headers_sent)
            .unwrap_or(false);
        if headers_sent {
            res_write(state, receiver, std::slice::from_ref(data))?;
        } else {
            let bytes = body_chunk(Some(data))?;
            if let Some(res) = state.borrow_mut().http.res.get_mut(&id) {
                res.body.extend_from_slice(&bytes);
            }
        }
    }
    let (status, text, headers, body, socket, keep_alive, http10, send_date, headers_sent, chunked) = {
        let guard = state.borrow();
        let Some(res) = guard.http.res.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        (
            res.status,
            res.text.clone(),
            res.headers.clone(),
            res.body.clone(),
            res.socket.clone(),
            res.keep_alive,
            res.http10,
            !matches!(
                execute::get_property(receiver.unwrap_or(&Value::Undefined), "sendDate"),
                Value::Boolean(false)
            ),
            res.headers_sent,
            res.chunked,
        )
    };
    let status = status_code(receiver, status);
    let keep_alive = response_keep_alive(&headers, keep_alive);
    if let Some(response) = receiver {
        let status_message = execute::get_property(response, "statusMessage");
        if matches!(status_message, Value::Undefined)
            || (matches!(status_message, Value::String(ref value) if value == "OK")
                && status != 200)
        {
            let message = match status as u16 {
                200 => "OK",
                201 => "Created",
                204 => "No Content",
                400 => "Bad Request",
                417 => "Expectation Failed",
                404 => "Not Found",
                _ => "",
            };
            execute::set_property_in_place(
                response,
                "statusMessage",
                Value::String(message.into()),
            );
        }
        if !matches!(
            execute::get_property(response, "\0quench:http-perf-recorded"),
            Value::Boolean(true)
        ) {
            let request = execute::get_property(response, "req");
            crate::modules::http::record_http_entry(
                state,
                "HttpRequest",
                request,
                response.clone(),
            );
            execute::set_property_in_place(
                response,
                "\0quench:http-perf-recorded",
                Value::Boolean(true),
            );
        }
    }
    let wire_text = receiver
        .and_then(|response| match execute::get_property(response, "statusMessage") {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or(text);
    if !headers_sent {
        let payload = host_api::bytes(&compose(
            status, &wire_text, &headers, &body, keep_alive, http10, send_date,
        ));
        crate::modules::net::socket_write(state, Some(&socket), std::slice::from_ref(&payload))?;
    } else if chunked {
        let terminator = host_api::bytes(b"0\r\n\r\n");
        crate::modules::net::socket_write(state, Some(&socket), std::slice::from_ref(&terminator))?;
    }
    if let Some(socket_id) = net::net_id(&socket) {
        if let Some(res) = state.borrow_mut().http.res.get_mut(&id) {
            res.ended = true;
        }
        if let Some(conn) = state.borrow_mut().http.conns.get_mut(&socket_id) {
            conn.response_done = true;
        }
        crate::modules::http::resume_connection(state, socket_id)?;
    }
    if let Some(response) = receiver {
        execute::set_property_in_place(response, "finished", Value::Boolean(true));
        execute::set_property_in_place(response, "writableEnded", Value::Boolean(true));
        execute::set_property_in_place(response, "destroyed", Value::Boolean(true));
        state
            .borrow_mut()
            .net
            .pending_events
            .push((response.clone(), "finish".into(), Vec::new()));
        // Node's ServerResponse closes as a writable stream completes; this
        // is independent of whether its HTTP socket is retained by keep-alive.
        if !matches!(
            execute::get_property(response, RESPONSE_CLOSE_PENDING_PROP),
            Value::Boolean(true)
        ) {
            execute::set_property_in_place(response, RESPONSE_CLOSE_PENDING_PROP, Value::Boolean(true));
            state
                .borrow_mut()
                .net
                .pending_events
                .push((response.clone(), "close".into(), Vec::new()));
        }
    }
    if !keep_alive {
        crate::modules::net::socket_end(state, Some(&socket), &[])?;
    }
    if let Some(callback) = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
        .or_else(|| {
            args.get(2)
                .filter(|value| quench_runtime::is_callable(value))
        })
    {
        execute::call(callback, receiver.unwrap_or(&Value::Undefined), &[])?;
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `res.destroy([error])` — close the response socket. The normal net close
/// transition notifies any in-flight request signal through `http::req_close`.
pub fn res_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, "destroyed", Value::Boolean(true));
        execute::set_property_in_place(receiver, "closed", Value::Boolean(true));
        if let Some(error) = args.first() {
            execute::set_property_in_place(receiver, "errored", error.clone());
        }
    }
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let socket = state
        .borrow()
        .http
        .res
        .get(&id)
        .map(|res| res.socket.clone());
    if let Some(socket) = socket {
        net::socket_destroy(state, Some(&socket), &[])?;
        state.borrow_mut().net.pending_events.push((
            receiver.cloned().unwrap_or(Value::Undefined),
            "close".into(),
            Vec::new(),
        ));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// `res.flushHeaders()` — send the current response head while leaving the
/// connection open for a later body or destroy transition.
pub fn res_flush_headers(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    };
    let (status, text, headers, socket, keep_alive, http10) = {
        let guard = state.borrow();
        let Some(res) = guard.http.res.get(&id) else {
            return Ok(receiver.cloned().unwrap_or(Value::Undefined));
        };
        (
            status_code(receiver, res.status),
            res.text.clone(),
            res.headers.clone(),
            res.socket.clone(),
            res.keep_alive,
            res.http10,
        )
    };
    let mut headers = headers;
    if !http10
        && !headers.iter().any(|(key, _)| {
            key.eq_ignore_ascii_case("content-length")
                || key.eq_ignore_ascii_case("transfer-encoding")
        })
    {
        // Flushing an unfinished keep-alive response must leave framing open;
        // Content-Length: 0 would make the client complete it immediately.
        headers.push(("transfer-encoding".into(), "chunked".into()));
    }
    let payload = host_api::bytes(&compose(
        status,
        &text,
        &headers,
        &[],
        keep_alive,
        http10,
        !matches!(
            execute::get_property(receiver.unwrap_or(&Value::Undefined), "sendDate"),
            Value::Boolean(false)
        ),
    ));
    net::socket_write(state, Some(&socket), std::slice::from_ref(&payload))?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// The effective status code, honoring a `res.statusCode = n` write.
fn status_code(receiver: Option<&Value>, default: u16) -> u16 {
    let Some(receiver) = receiver else {
        return default;
    };
    match quench_runtime::vm::get_property(receiver, "statusCode") {
        Value::Number(n) if n.is_finite() && (100.0..600.0).contains(&n) => n as u16,
        _ => default,
    }
}

/// Serialize an HTTP/1.1 response.
fn compose(
    status: u16,
    text: &str,
    headers: &[(String, String)],
    body: &[u8],
    keep_alive: bool,
    http10: bool,
    send_date: bool,
) -> Vec<u8> {
    let text = if text.is_empty() { "OK" } else { text };
    let mut out = format!("HTTP/1.1 {status} {text}\r\n").into_bytes();
    let chunked = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    if !http10
        && !chunked
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
    {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    if send_date
        && !headers
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("date"))
    {
        out.extend_from_slice(b"Date: Thu, 01 Jan 1970 00:00:00 GMT\r\n");
    }
    let connection = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("connection"))
        .map(|(_, value)| value.as_str())
        .unwrap_or(if keep_alive { "keep-alive" } else { "close" });
    let transfer_encoding = headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
        .collect::<Vec<_>>();
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("Connection: {connection}\r\n").as_bytes());
    for (key, value) in transfer_encoding {
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if chunked {
        out.extend_from_slice(&chunk_frame(body));
    } else {
        out.extend_from_slice(body);
    }
    out
}

fn response_keep_alive(headers: &[(String, String)], fallback: bool) -> bool {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("connection"))
        .map(|(_, value)| !value.eq_ignore_ascii_case("close"))
        .unwrap_or(fallback)
}

fn chunk_frame(body: &[u8]) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut frame = format!("{:x}\r\n", body.len()).into_bytes();
    frame.extend_from_slice(body);
    frame.extend_from_slice(b"\r\n");
    frame
}

fn valid_status(value: &Value) -> Option<u16> {
    match value {
        Value::Number(number)
            if number.is_finite() && number.fract() == 0.0 && (100.0..=999.0).contains(number) =>
        {
            Some(*number as u16)
        }
        _ => None,
    }
}

fn invalid_status(value: &Value) -> VmError {
    let rendered = match value {
        Value::Array(_) => "[]".to_string(),
        Value::Object(_) | Value::ObjectAlias(_) => "{}".to_string(),
        _ => execute::to_js_string(value).unwrap_or_else(|_| "undefined".into()),
    };
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::RangeError,
        &[Value::String(format!("Invalid status code: {rendered}"))],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_HTTP_INVALID_STATUS_CODE".into()),
    ))
}
