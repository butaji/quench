//! Shared-VM Fetch entry backed by real HTTP(S) requests.
//!
//! The worker owns only serialized request/response data and OS I/O. Promise
//! roots and response objects stay on the VM thread and are delivered at the
//! shared event-loop checkpoint.

use crate::host::{HostState, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

const FIRST_FETCH_ID: u64 = 1;
const FETCH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FETCH_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FETCH_REDIRECTS: usize = 20;
const SHARED_IO_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Clone)]
struct FetchRequest {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct FetchResponse {
    url: String,
    redirected: bool,
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct FetchCompletion {
    id: u64,
    result: Result<FetchResponse, String>,
}

struct PendingFetch {
    resolve: RootId,
    reject: RootId,
}

pub struct FetchState {
    next_id: u64,
    requests: HashMap<u64, FetchRequest>,
    pending: HashMap<u64, PendingFetch>,
    completions: HashMap<u64, Result<FetchResponse, String>>,
    sender: Sender<FetchCompletion>,
    receiver: Receiver<FetchCompletion>,
}

impl FetchState {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            next_id: FIRST_FETCH_ID,
            requests: HashMap::new(),
            pending: HashMap::new(),
            completions: HashMap::new(),
            sender,
            receiver,
        }
    }

    fn reserve(&mut self, request: FetchRequest) -> Result<u64, RootedError> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| RootedError::host("Fetch request identifier space exhausted"))?;
        self.requests.insert(id, request);
        Ok(id)
    }

    fn pending(&self) -> bool {
        !self.requests.is_empty() || !self.pending.is_empty()
    }
}

pub(crate) fn fetch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let request = request_from_args(context, args)?;
    let id = context
        .host_mut()
        .state()
        .borrow_mut()
        .fetch
        .reserve(request)?;
    let id_data = context.number(id as f64);
    let executor = context
        .host_function_with_data(crate::host::shared_vm::operation("fetchExecutor"), id_data)?;
    let global = context.global_root()?;
    let promise_key = context.string_rooted("Promise");
    let promise = context.get_property_rooted(global, promise_key)?;
    match context.construct_rooted(promise, promise, &[executor]) {
        Ok(promise) => Ok(promise),
        Err(error) => {
            context
                .host_mut()
                .state()
                .borrow_mut()
                .fetch
                .requests
                .remove(&id);
            Err(error)
        }
    }
}

fn request_from_args(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<FetchRequest, RootedError> {
    let input = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("Fetch input root is missing"))?;
    let input_url = property(context, input, "url")?;
    let url_root = if input_url
        .and_then(|root| context.rooted_value(root))
        .is_some_and(|value| !value.is_undefined())
    {
        input_url.expect("checked URL property")
    } else {
        input
    };
    let url = context.to_string(url_root)?;

    let request_method = property(context, input, "method")?;
    let init = args.get(1).copied();
    let init_method = init
        .map(|init| property(context, init, "method"))
        .transpose()?
        .flatten();
    let method_root = init_method.or(request_method).filter(|root| {
        context
            .rooted_value(*root)
            .is_some_and(|value| !value.is_undefined())
    });
    let method = method_root
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_else(|| "GET".to_owned());
    if !is_token(&method) || matches!(method.as_str(), "CONNECT" | "TRACE" | "TRACK") {
        return Err(type_error(context, "Request method is invalid"));
    }

    let request_headers = property(context, input, "headers")?;
    let init_headers = init
        .map(|init| property(context, init, "headers"))
        .transpose()?
        .flatten();
    let headers_root = init_headers
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined())
        })
        .or(request_headers);
    let headers = headers_root
        .map(|root| headers_from_object(context, root))
        .transpose()?
        .unwrap_or_default();

    let request_body = property(context, input, "body")?;
    let init_body = init
        .map(|init| property(context, init, "body"))
        .transpose()?
        .flatten();
    let body_root = init_body
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined())
        })
        .or(request_body)
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined() && !value.is_null())
        });
    if body_root.is_some() && matches!(method.as_str(), "GET" | "HEAD") {
        return Err(type_error(
            context,
            "Request with GET/HEAD method cannot have body",
        ));
    }
    let body = body_root
        .map(|root| context.to_string(root).map(String::into_bytes))
        .transpose()?
        .unwrap_or_default();

    Ok(FetchRequest {
        url,
        method,
        headers,
        body,
    })
}

fn headers_from_object(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
) -> Result<Vec<(String, String)>, RootedError> {
    let global = context.global_root()?;
    let object_key = context.string_rooted("Object");
    let object_constructor = context.get_property_rooted(global, object_key)?;
    let keys_key = context.string_rooted("keys");
    let keys_function = context.get_property_rooted(object_constructor, keys_key)?;
    let keys = context.call_rooted(keys_function, object_constructor, &[object])?;
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(keys, length_key)?;
    let count = context
        .rooted_value(length)
        .and_then(Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0)
        .map(|length| length.trunc() as usize)
        .unwrap_or(0);
    let mut headers = BTreeMap::<String, String>::new();
    for index in 0..count {
        let key_index = context.string_rooted(&index.to_string());
        let name_root = context.get_property_rooted(keys, key_index)?;
        let raw_name = context.to_string(name_root)?;
        let name = raw_name.to_ascii_lowercase();
        let key = context.string_rooted(&raw_name);
        let value_root = context.get_property_rooted(object, key)?;
        let value = context.to_string(value_root)?;
        if !is_token(&name) || contains_line_break(&value) {
            return Err(type_error(context, "Invalid HTTP header"));
        }
        headers
            .entry(name)
            .and_modify(|previous| {
                previous.push_str(", ");
                previous.push_str(&value);
            })
            .or_insert(value);
    }
    Ok(headers.into_iter().collect())
}

pub(crate) fn executor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id_root = context.host_function_data()?;
    let id = context
        .rooted_value(id_root)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid Fetch request id"))?;
    let (request, sender) = {
        let state = context.host_mut().state();
        let mut host = state.borrow_mut();
        let request = host
            .fetch
            .requests
            .remove(&id)
            .ok_or_else(|| RootedError::host("Fetch request was not registered"))?;
        let resolve = args
            .first()
            .copied()
            .ok_or_else(|| RootedError::host("Fetch resolve callback is missing"))?;
        let reject = args
            .get(1)
            .copied()
            .ok_or_else(|| RootedError::host("Fetch reject callback is missing"))?;
        let resolve = context.retain(resolve)?;
        let reject = context.retain(reject)?;
        host.fetch
            .pending
            .insert(id, PendingFetch { resolve, reject });
        (request, host.fetch.sender.clone())
    };
    let spawned = std::thread::Builder::new()
        .name(format!("quench-fetch-{id}"))
        .spawn(move || {
            let result = perform_fetch(request);
            let _ = sender.send(FetchCompletion { id, result });
        });
    if let Err(error) = spawned {
        let state = context.host_mut().state();
        let pending = state.borrow_mut().fetch.pending.remove(&id);
        if let Some(pending) = pending {
            context.release_root(pending.resolve);
            context.release_root(pending.reject);
        }
        return Err(RootedError::host(format!(
            "cannot start Fetch worker: {error}"
        )));
    }
    Ok(context.undefined())
}

pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    state: &std::rc::Rc<std::cell::RefCell<HostState>>,
) -> Result<bool, String> {
    let mut completions = Vec::new();
    loop {
        let received = state.borrow().fetch.receiver.try_recv();
        match received {
            Ok(completion) => {
                completions.push(completion.id);
                state
                    .borrow_mut()
                    .fetch
                    .completions
                    .insert(completion.id, completion.result);
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    if completions.is_empty() {
        return Ok(false);
    }
    let operation = runtime
        .host_function(crate::host::shared_vm::operation("fetchComplete"))
        .map_err(|error| error.to_string())?;
    let receiver = runtime.root(Value::UNDEFINED);
    for id in completions {
        let argument = runtime.root(Value::number(id as f64));
        let result = runtime.call_rooted(operation, receiver, &[argument]);
        runtime.release_root(argument);
        match result {
            Ok(result) => {
                runtime.release_root(result);
                runtime
                    .run_host_jobs(program)
                    .map_err(|error| runtime.format_error(program, &error))?;
            }
            Err(error) => return Err(runtime.format_error(program, &error.error)),
        }
    }
    runtime.release_root(operation);
    runtime.release_root(receiver);
    Ok(true)
}

pub(crate) fn has_pending(state: &std::rc::Rc<std::cell::RefCell<HostState>>) -> bool {
    state.borrow().fetch.pending()
}

pub(crate) fn poll_interval() -> Duration {
    SHARED_IO_POLL_INTERVAL
}

pub(crate) fn complete(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid Fetch completion id"))?;
    let pending = context
        .host_mut()
        .state()
        .borrow_mut()
        .fetch
        .pending
        .remove(&id)
        .ok_or_else(|| RootedError::host("Fetch completion has no pending promise"))?;
    let result = context
        .host_mut()
        .state()
        .borrow_mut()
        .fetch
        .completions
        .remove(&id)
        .ok_or_else(|| RootedError::host("Fetch completion result is missing"))?;
    let (target, value) = match result {
        Ok(response) => match response_object(context, response) {
            Ok(value) => (pending.resolve, value),
            Err(error) => {
                context.release_root(pending.resolve);
                context.release_root(pending.reject);
                return Err(error);
            }
        },
        Err(message) => {
            let error = context.error_rooted(&message)?;
            (pending.reject, error)
        }
    };
    let receiver = context.undefined();
    let result = context.call_rooted(target, receiver, &[value]);
    context.release_root(pending.resolve);
    context.release_root(pending.reject);
    result
}

fn response_object(
    context: &mut NativeContext<'_, NodeHost>,
    response: FetchResponse,
) -> Result<RootId, RootedError> {
    let body = String::from_utf8_lossy(&response.body).into_owned();
    let object = context.object_rooted()?;
    set_number(context, object, "status", response.status as f64)?;
    set_text(context, object, "statusText", &response.status_text)?;
    set_text(context, object, "url", &response.url)?;
    set_bool(context, object, "ok", (200..300).contains(&response.status))?;
    set_bool(context, object, "redirected", response.redirected)?;
    set_bool(context, object, "bodyUsed", false)?;
    let null = context.null();
    set(context, object, "body", null)?;

    let headers = context.object_rooted()?;
    for (name, value) in response.headers {
        set_text(context, headers, &name, &value)?;
    }
    install_method(context, headers, "get", "fetchHeaderGet")?;
    install_method(context, headers, "has", "fetchHeaderHas")?;
    set(context, object, "headers", headers)?;

    let body = context.string_rooted(&body);
    install_data_method(context, object, "text", "fetchResponseText", body)?;
    install_data_method(context, object, "json", "fetchResponseJson", body)?;
    Ok(object)
}

pub(crate) fn response_text(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let body = context.host_function_data()?;
    let promise = resolved_promise(context, body)?;
    Ok(promise)
}

pub(crate) fn response_json(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let body = context.host_function_data()?;
    let text = context.string_text(body)?.unwrap_or_default();
    match context.parse_json_rooted(&text) {
        Ok(value) => resolved_promise(context, value),
        Err(error) => {
            if let Some(exception) = error.exception {
                rejected_promise(context, exception)
            } else {
                Err(error)
            }
        }
    }
}

pub(crate) fn header_get(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(name) = args.first().copied() else {
        return Ok(context.null());
    };
    let name = context.to_string(name)?.to_ascii_lowercase();
    let key = context.string_rooted(&name);
    let value = context.get_property_rooted(receiver, key)?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        Ok(context.null())
    } else {
        Ok(value)
    }
}

pub(crate) fn header_has(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(name) = args.first().copied() else {
        return Ok(context.boolean(false));
    };
    let name = context.to_string(name)?.to_ascii_lowercase();
    let key = context.string_rooted(&name);
    let value = context.get_property_rooted(receiver, key)?;
    Ok(context.boolean(
        !context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined()),
    ))
}

fn resolved_promise(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    promise_method(context, "resolve", value)
}

fn rejected_promise(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    promise_method(context, "reject", value)
}

fn promise_method(
    context: &mut NativeContext<'_, NodeHost>,
    method: &str,
    value: RootId,
) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let promise_key = context.string_rooted("Promise");
    let promise = context.get_property_rooted(global, promise_key)?;
    let method_key = context.string_rooted(method);
    let method = context.get_property_rooted(promise, method_key)?;
    context.call_rooted(method, promise, &[value])
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<RootId>, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key).map(Some)
}

fn install_method(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    operation: &str,
) -> Result<(), RootedError> {
    let function = context.host_function(crate::host::shared_vm::operation(operation))?;
    set(context, object, name, function)
}

fn install_data_method(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    operation: &str,
    data: RootId,
) -> Result<(), RootedError> {
    let function =
        context.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
    set(context, object, name, function)
}

fn set_text(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set_number(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), RootedError> {
    let value = context.number(value);
    set(context, object, name, value)
}

fn set_bool(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: bool,
) -> Result<(), RootedError> {
    let value = context.boolean(value);
    set(context, object, name, value)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install Fetch property {name}"
        )))
    }
}

fn type_error(context: &mut NativeContext<'_, NodeHost>, message: &str) -> RootedError {
    match context.type_error_rooted(message) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

fn perform_fetch(mut request: FetchRequest) -> Result<FetchResponse, String> {
    let initial_url = request.url.clone();
    for redirect_count in 0..=MAX_FETCH_REDIRECTS {
        let target = url::Url::parse(&request.url).map_err(|error| error.to_string())?;
        let response = transact(&target, &request)?;
        let location = response
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .map(|(_, value)| value.clone());
        let is_redirect = matches!(response.status, 301 | 302 | 303 | 307 | 308);
        if !is_redirect || location.is_none() {
            return Ok(FetchResponse {
                url: request.url,
                redirected: redirect_count != 0,
                status: response.status,
                status_text: response.status_text,
                headers: response.headers,
                body: response.body,
            });
        }
        if redirect_count == MAX_FETCH_REDIRECTS {
            return Err("Fetch redirect limit exceeded".into());
        }
        let next = target
            .join(location.as_deref().unwrap_or_default())
            .map_err(|error| error.to_string())?;
        if response.status == 303
            || matches!(response.status, 301 | 302) && request.method == "POST"
        {
            request.method = "GET".into();
            request.body.clear();
            request
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
        }
        request.url = next.to_string();
    }
    Err(format!("Fetch redirect loop from {initial_url}"))
}

fn transact(target: &url::Url, request: &FetchRequest) -> Result<FetchResponse, String> {
    let scheme = target.scheme();
    if !matches!(scheme, "http" | "https") {
        return Err(format!("Fetch scheme {scheme:?} is not supported"));
    }
    let host = target
        .host_str()
        .ok_or_else(|| "Fetch URL has no host".to_owned())?;
    let port = target
        .port_or_known_default()
        .ok_or_else(|| "Fetch URL has no port".to_owned())?;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("Fetch DNS lookup failed: {error}"))?
        .collect::<Vec<SocketAddr>>();
    let mut last_error = None;
    let mut connected = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, FETCH_CONNECT_TIMEOUT) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let stream = connected.ok_or_else(|| {
        last_error.map_or_else(
            || "Fetch DNS lookup returned no addresses".to_owned(),
            |error| format!("Fetch connection failed: {error}"),
        )
    })?;
    stream
        .set_read_timeout(Some(FETCH_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(FETCH_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;

    let path = if target.path().is_empty() {
        "/".to_owned()
    } else {
        target.path().to_owned()
    } + target
        .query()
        .map(|query| format!("?{query}"))
        .as_deref()
        .unwrap_or("");
    let mut headers = request
        .headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    headers
        .entry("host".into())
        .or_insert_with(|| authority(target));
    headers
        .entry("connection".into())
        .or_insert_with(|| "close".into());
    if !request.body.is_empty() {
        headers
            .entry("content-length".into())
            .or_insert_with(|| request.body.len().to_string());
    }
    let mut wire = format!("{} {path} HTTP/1.1\r\n", request.method);
    for (name, value) in &headers {
        wire.push_str(name);
        wire.push_str(": ");
        wire.push_str(value);
        wire.push_str("\r\n");
    }
    wire.push_str("\r\n");
    let mut request_bytes = wire.into_bytes();
    request_bytes.extend_from_slice(&request.body);

    let bytes = if scheme == "https" {
        let connector = openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls())
            .map_err(|error| format!("Fetch TLS setup failed: {error}"))?
            .build();
        let mut stream = connector
            .connect(host, stream)
            .map_err(|error| format!("Fetch TLS handshake failed: {error}"))?;
        stream
            .write_all(&request_bytes)
            .map_err(|error| format!("Fetch request write failed: {error}"))?;
        read_limited(&mut stream)?
    } else {
        let mut stream = stream;
        stream
            .write_all(&request_bytes)
            .map_err(|error| format!("Fetch request write failed: {error}"))?;
        read_limited(&mut stream)?
    };
    parse_response(bytes, request.method == "HEAD")
}

fn read_limited(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_FETCH_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Fetch response read failed: {error}"))?;
    if bytes.len() > MAX_FETCH_RESPONSE_BYTES {
        return Err("Fetch response exceeded the configured body limit".into());
    }
    Ok(bytes)
}

fn parse_response(mut bytes: Vec<u8>, head_only: bool) -> Result<FetchResponse, String> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "Fetch response did not contain an HTTP header terminator".to_owned())?;
    let body = bytes.split_off(split + 4);
    bytes.truncate(split);
    let header_text = std::str::from_utf8(&bytes)
        .map_err(|_| "Fetch response headers were not valid ASCII/UTF-8".to_owned())?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "Fetch response status line is missing".to_owned())?;
    let mut status_parts = status_line.splitn(3, ' ');
    let version = status_parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err("Fetch response status line has an invalid protocol".into());
    }
    let status = status_parts
        .next()
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| "Fetch response status code is invalid".to_owned())?;
    let status_text = status_parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::<(String, String)>::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err("Fetch response contains a malformed header".into());
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if let Some((_, previous)) = headers.iter_mut().find(|(existing, _)| *existing == name) {
            previous.push_str(", ");
            previous.push_str(&value);
        } else {
            headers.push((name, value));
        }
    }
    let chunked = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|coding| coding.trim().eq_ignore_ascii_case("chunked"))
    });
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok());
    let body = if head_only || status == 204 || status == 304 {
        Vec::new()
    } else if chunked {
        decode_chunked(&body)?
    } else if let Some(length) = content_length {
        body.into_iter().take(length).collect()
    } else {
        body
    };
    Ok(FetchResponse {
        url: String::new(),
        redirected: false,
        status,
        status_text,
        headers,
        body,
    })
}

fn decode_chunked(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_end = bytes[offset..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .map(|length| offset + length)
            .ok_or_else(|| "Malformed chunked Fetch response".to_owned())?;
        let line = std::str::from_utf8(&bytes[offset..line_end])
            .map_err(|_| "Malformed chunk size".to_owned())?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or_default(), 16)
            .map_err(|_| "Malformed chunk size".to_owned())?;
        offset = line_end + 2;
        if size == 0 {
            return Ok(result);
        }
        let end = offset
            .checked_add(size)
            .filter(|end| *end + 2 <= bytes.len())
            .ok_or_else(|| "Truncated chunked Fetch response".to_owned())?;
        result.extend_from_slice(&bytes[offset..end]);
        if &bytes[end..end + 2] != b"\r\n" {
            return Err("Malformed chunk terminator".into());
        }
        offset = end + 2;
    }
}

fn authority(target: &url::Url) -> String {
    let host = target.host_str().unwrap_or_default();
    let default_port = match target.scheme() {
        "http" => 80,
        "https" => 443,
        _ => 0,
    };
    match target.port() {
        Some(port) if port != default_port => format!("{host}:{port}"),
        _ => host.to_owned(),
    }
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn contains_line_break(text: &str) -> bool {
    text.bytes().any(|byte| byte == b'\r' || byte == b'\n')
}
