//! Server-side HTTP response (`res`) methods, split out of `http` to
//! keep both files under the line limit. State lives in the `res` map
//! on `http::HttpState`, keyed by a hidden `res` object property.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

use super::http::{chunk_bytes, Res, RES_ID_PROP};
use crate::modules::net;

fn res_state(receiver: Option<&Value>) -> Option<u64> {
    let receiver = receiver?;
    match quench_runtime::vm::get_property(receiver, RES_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as u64),
        _ => None,
    }
}

/// `res.setHeader(name, value)` — replace any existing header of that name.
pub fn res_set_header(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(id) = res_state(receiver) else {
        return Ok(Value::Undefined);
    };
    let name = args.first().map(execute::to_js_string).transpose()?;
    let value = args.get(1).map(execute::to_js_string).transpose()?;
    let Some((name, value)) = name.zip(value) else {
        return Ok(Value::Undefined);
    };
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.headers.retain(|(key, _)| key != &name);
        res.headers.push((name, value));
    }
    Ok(Value::Undefined)
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
    let status = args.first().and_then(number).unwrap_or(200).clamp(100, 599);
    if let Some(Value::Object(_)) = args.get(1) {
        let object = args.get(1).cloned().unwrap_or(Value::Undefined);
        let mut guard = state.borrow_mut();
        if let Some(res) = guard.http.res.get_mut(&id) {
            res.status = status;
            merge_headers(res, &object)?;
        }
        return Ok(Value::Undefined);
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
    Ok(Value::Undefined)
}

fn merge_headers(res: &mut Res, object: &Value) -> Result<(), VmError> {
    res.headers.clear();
    for key in execute::own_enumerable_keys(object) {
        if let Ok(item) = execute::get_property_result(object, &key) {
            if let Ok(value) = execute::to_js_string(&item) {
                res.headers.push((key, value));
            }
        }
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
        chunk_bytes(Some(value))
    };
    let mut guard = state.borrow_mut();
    if let Some(res) = guard.http.res.get_mut(&id) {
        res.body.extend_from_slice(&bytes);
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
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
    if let Some(data) = args.first() {
        if !matches!(data, Value::Undefined) {
            res_write(state, receiver, std::slice::from_ref(data))?;
        }
    }
    let (status, text, headers, body, socket, keep_alive) = {
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
        )
    };
    let status = status_code(receiver, status);
    let payload = host_api::bytes(&compose(status, &text, &headers, &body, keep_alive));
    crate::modules::net::socket_write(state, Some(&socket), std::slice::from_ref(&payload))?;
    if keep_alive {
        // Leave the socket open and let the connection parser reset for the
        // next request on the same connection.
        if let Some(socket_id) = net::net_id(&socket) {
            if let Some(conn) = state.borrow_mut().http.conns.get_mut(&socket_id) {
                conn.response_done = true;
            }
        }
    } else {
        crate::modules::net::socket_end(state, Some(&socket), &[])?;
    }
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
) -> Vec<u8> {
    let text = if text.is_empty() { "OK" } else { text };
    let mut out = format!("HTTP/1.1 {status} {text}\r\n").into_bytes();
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    if !headers
        .iter()
        .any(|(key, _)| key.eq_ignore_ascii_case("date"))
    {
        out.extend_from_slice(b"Date: Thu, 01 Jan 1970 00:00:00 GMT\r\n");
    }
    for (key, value) in headers {
        out.extend_from_slice(format!("{key}: {value}\r\n").as_bytes());
    }
    let connection = if keep_alive { "keep-alive" } else { "close" };
    out.extend_from_slice(format!("Connection: {connection}\r\n").as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn number(value: &Value) -> Option<u16> {
    match value {
        Value::Number(n) if n.is_finite() => Some(*n as u16),
        _ => None,
    }
}
