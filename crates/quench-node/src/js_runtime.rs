use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use quench_runtime::{
    ops::{HostCapabilityKind, HostCapabilityRef, RealmId},
    value::Value,
    vm::{Host, VmContext, VmError},
};

pub(crate) struct FilesystemNodeHost;

impl NodeHost for FilesystemNodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let base = parent
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        Ok(if request.starts_with('.') {
            base.join(request)
        } else {
            PathBuf::from(request)
        })
    }

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        Ok(std::fs::read_to_string(path)?)
    }
}

pub(crate) trait NodeHost {
    fn resolve_module(
        &self,
        request: &str,
        parent: Option<&Path>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>>;

    fn load_module(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>>;
}

pub(crate) trait JsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>>;

    fn has_pending_jobs(&self) -> bool;
}

pub(crate) struct QuenchRuntime;

struct QuenchNodeHost {
    hashes: RefCell<HashMap<u16, Vec<u8>>>,
    streams: RefCell<HashMap<u16, StreamState>>,
    next_hash: Cell<u16>,
    next_stream: Cell<u16>,
    http: RefCell<HttpState>,
    urls: RefCell<HashMap<u16, String>>,
    next_url: Cell<u16>,
}

struct StreamState {
    transform: Option<Value>,
    data: Option<Value>,
    end: Option<Value>,
}

struct HttpState {
    server_callback: Option<Value>,
    body: String,
    data_callback: Option<Value>,
    end_callback: Option<Value>,
}

impl Default for QuenchNodeHost {
    fn default() -> Self {
        Self {
            hashes: RefCell::new(HashMap::new()),
            streams: RefCell::new(HashMap::new()),
            next_hash: Cell::new(100),
            next_stream: Cell::new(200),
            http: RefCell::new(HttpState {
                server_callback: None,
                body: String::new(),
                data_callback: None,
                end_callback: None,
            }),
            urls: RefCell::new(HashMap::new()),
            next_url: Cell::new(600),
        }
    }
}

impl Host for QuenchNodeHost {
    fn call(&self, capability: HostCapabilityRef, arguments: &[Value]) -> Result<Value, VmError> {
        match capability.kind {
            HostCapabilityKind::Custom(1) => require_module(arguments),
            HostCapabilityKind::Custom(2) => basename(arguments),
            HostCapabilityKind::Custom(4) => console_log(arguments),
            HostCapabilityKind::Custom(6) => current_directory(arguments),
            HostCapabilityKind::Custom(7) => read_file_sync(arguments),
            HostCapabilityKind::Custom(8) => self.create_hash(arguments),
            HostCapabilityKind::Custom(9) => buffer_byte_length(arguments),
            HostCapabilityKind::Custom(30) => buffer_from(arguments),
            HostCapabilityKind::Custom(31) => buffer_alloc(arguments),
            HostCapabilityKind::Custom(32) => buffer_is_buffer(arguments),
            HostCapabilityKind::Custom(80) => util_format(arguments),
            HostCapabilityKind::Custom(81) => util_inspect(arguments),
            HostCapabilityKind::Custom(82..=85) => os_value(capability.kind),
            HostCapabilityKind::Custom(id) if (600..700).contains(&id) => self.url_call(id),
            HostCapabilityKind::Custom(21) => next_tick(arguments),
            HostCapabilityKind::Custom(27 | 28 | 29) => timer_call(arguments),
            HostCapabilityKind::Custom(id) if (13..=20).contains(&id) => {
                assertion_call(id, arguments)
            }
            HostCapabilityKind::Custom(11 | 12) => self.http_call(capability.kind, arguments),
            HostCapabilityKind::Custom(id) if (400..600).contains(&id) => {
                self.http_call(capability.kind, arguments)
            }
            HostCapabilityKind::Custom(id) if (200..300).contains(&id) => {
                self.stream_call(id, arguments)
            }
            HostCapabilityKind::Custom(id) if id >= 100 => self.hash_call(id, arguments),
            _ => Err(VmError::NotCallable),
        }
    }

    fn construct(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        if capability.kind == HostCapabilityKind::Custom(40) {
            let input = match arguments.first() {
                Some(Value::String(value)) => value.as_str(),
                _ => return Err(VmError::EvalError("URL expects a string".into())),
            };
            let parsed = match arguments.get(1) {
                Some(Value::String(base)) => {
                    url::Url::parse(base).and_then(|base| base.join(input))
                }
                _ => url::Url::parse(input),
            }
            .map_err(|error| VmError::EvalError(error.to_string()))?;
            let id = self.next_url.get();
            self.next_url.set(id.saturating_add(1));
            self.urls.borrow_mut().insert(id, parsed.to_string());
            return Ok(url_object(&parsed, id));
        }
        if capability.kind != HostCapabilityKind::Custom(10) {
            return Err(VmError::NotCallable);
        }
        let id = self.next_stream.get();
        self.next_stream.set(id.saturating_add(10));
        let transform = arguments
            .first()
            .and_then(|options| {
                quench_runtime::execute::get_property_result(options, "transform").ok()
            })
            .filter(|value| !matches!(value, Value::Undefined));
        self.streams.borrow_mut().insert(
            id,
            StreamState {
                transform,
                data: None,
                end: None,
            },
        );
        Ok(Value::object(vec![
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(id + 2)),
            ),
        ]))
    }
}

impl QuenchNodeHost {
    fn url_call(&self, id: u16) -> Result<Value, VmError> {
        let value = self
            .urls
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        Ok(Value::String(value))
    }

    fn stream_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let stream_id = id / 10 * 10;
        let operation = id % 10;
        match operation {
            1 => {
                let Some(Value::String(event)) = arguments.first() else {
                    return Err(VmError::EvalError("stream.on expects an event".into()));
                };
                let Some(callback) = arguments.get(1) else {
                    return Err(VmError::EvalError("stream.on expects a callback".into()));
                };
                let mut streams = self.streams.borrow_mut();
                let state = streams.get_mut(&stream_id).ok_or(VmError::NotCallable)?;
                match event.as_str() {
                    "data" => state.data = Some(callback.clone()),
                    "end" => state.end = Some(callback.clone()),
                    _ => {}
                }
                Ok(capability_function(HostCapabilityKind::Custom(stream_id)))
            }
            2 => {
                let chunk = string_or_bytes(arguments.first())?;
                let transform = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.transform.clone())
                    .ok_or(VmError::NotCallable)?;
                let callback = capability_function(HostCapabilityKind::Custom(stream_id + 3));
                quench_runtime::execute::call(
                    &transform,
                    &Value::Undefined,
                    &[
                        Value::String(String::from_utf8_lossy(&chunk).into_owned()),
                        Value::String("buffer".into()),
                        callback,
                    ],
                )?;
                Ok(Value::Undefined)
            }
            3 => {
                let output = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                if !matches!(output, Value::Null | Value::Undefined) {
                    if let Some(data) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|s| s.data.clone())
                    {
                        quench_runtime::execute::call(&data, &Value::Undefined, &[output])?;
                    }
                }
                if let Some(end) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|s| s.end.clone())
                {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            4 => {
                if let Some(end) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|s| s.end.clone())
                {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            _ => Err(VmError::NotCallable),
        }
    }

    fn http_call(&self, kind: HostCapabilityKind, arguments: &[Value]) -> Result<Value, VmError> {
        match kind {
            HostCapabilityKind::Custom(11) => {
                self.http.borrow_mut().server_callback = arguments.first().cloned();
                Ok(Value::object(vec![
                    (
                        "listen".into(),
                        capability_function(HostCapabilityKind::Custom(401)),
                    ),
                    (
                        "address".into(),
                        capability_function(HostCapabilityKind::Custom(402)),
                    ),
                    (
                        "close".into(),
                        capability_function(HostCapabilityKind::Custom(403)),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(12) => {
                let url = arguments
                    .first()
                    .and_then(|v| match v {
                        Value::String(s) => Some(s),
                        _ => None,
                    })
                    .ok_or(VmError::NotCallable)?;
                let callback = arguments.get(1).cloned().ok_or(VmError::NotCallable)?;
                let path = url
                    .split('/')
                    .skip(3)
                    .next()
                    .map(|p| format!("/{p}"))
                    .unwrap_or_else(|| "/".into());
                let response = response_object(500);
                let request = Value::object(vec![("url".into(), Value::String(path))]);
                let server = self
                    .http
                    .borrow()
                    .server_callback
                    .clone()
                    .ok_or(VmError::NotCallable)?;
                quench_runtime::execute::call(
                    &server,
                    &Value::Undefined,
                    &[request, response.clone()],
                )?;
                quench_runtime::execute::call(&callback, &Value::Undefined, &[response])?;
                let state = self.http.borrow();
                if let Some(data) = state.data_callback.clone() {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[Value::String(state.body.clone())],
                    )?;
                }
                if let Some(end) = state.end_callback.clone() {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(401) => {
                if let Some(callback) = arguments.last() {
                    quench_runtime::execute::call(callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(402) => {
                Ok(Value::object(vec![("port".into(), Value::Number(43123.0))]))
            }
            HostCapabilityKind::Custom(403) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(id) if (500..600).contains(&id) => {
                match id % 10 {
                    4 => {
                        self.http.borrow_mut().body =
                            arguments.first().map(value_to_string).unwrap_or_default()
                    }
                    5 => {
                        if matches!(arguments.first(), Some(Value::String(event)) if event == "data")
                        {
                            self.http.borrow_mut().data_callback = arguments.get(1).cloned();
                        } else if matches!(arguments.first(), Some(Value::String(event)) if event == "end")
                        {
                            self.http.borrow_mut().end_callback = arguments.get(1).cloned();
                        }
                    }
                    _ => {}
                }
                Ok(Value::Undefined)
            }
            _ => Err(VmError::NotCallable),
        }
    }
}

fn response_object(base: u16) -> Value {
    Value::object(vec![
        (
            "end".into(),
            capability_function(HostCapabilityKind::Custom(base + 4)),
        ),
        (
            "on".into(),
            capability_function(HostCapabilityKind::Custom(base + 5)),
        ),
        (
            "setEncoding".into(),
            capability_function(HostCapabilityKind::Custom(base + 6)),
        ),
    ])
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => format!("{other:?}"),
    }
}

impl QuenchNodeHost {
    fn create_hash(&self, arguments: &[Value]) -> Result<Value, VmError> {
        if !matches!(arguments.first(), Some(Value::String(name)) if name == "sha256") {
            return Err(VmError::EvalError("only sha256 is supported".into()));
        }
        let id = self.next_hash.get();
        self.next_hash.set(id.saturating_add(2));
        self.hashes.borrow_mut().insert(id, Vec::new());
        Ok(Value::object(vec![
            (
                "update".into(),
                capability_function(HostCapabilityKind::Custom(id)),
            ),
            (
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
        ]))
    }

    fn hash_call(&self, id: u16, arguments: &[Value]) -> Result<Value, VmError> {
        let base = id - (id % 2);
        if id % 2 == 0 {
            let value = string_or_bytes(arguments.first())?;
            self.hashes
                .borrow_mut()
                .entry(base)
                .or_default()
                .extend(value);
            return Ok(Value::object(vec![(
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            )]));
        }
        let data = self.hashes.borrow().get(&base).cloned().unwrap_or_default();
        let digest = Sha256::digest(data);
        if matches!(arguments.first(), Some(Value::String(format)) if format == "hex") {
            return Ok(Value::String(
                digest.iter().map(|byte| format!("{byte:02x}")).collect(),
            ));
        }
        Ok(Value::String(String::from_utf8_lossy(&digest).into_owned()))
    }
}

fn console_log(arguments: &[Value]) -> Result<Value, VmError> {
    let line = arguments
        .iter()
        .map(|value| match value {
            Value::String(value) => value.clone(),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("{line}");
    Ok(Value::Undefined)
}

fn current_directory(arguments: &[Value]) -> Result<Value, VmError> {
    if !arguments.is_empty() {
        return Err(VmError::EvalError(
            "process.cwd expects no arguments".into(),
        ));
    }
    Ok(Value::String(
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned(),
    ))
}

fn read_file_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("readFileSync expects a path".into()));
    };
    let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    if matches!(arguments.get(1), Some(Value::String(encoding)) if encoding == "utf8") {
        return String::from_utf8(bytes)
            .map(Value::String)
            .map_err(|error| VmError::EvalError(error.to_string()));
    }
    Ok(quench_runtime::host_api::bytes(&bytes))
}

fn string_or_bytes(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    match value {
        Some(Value::String(value)) => Ok(value.as_bytes().to_vec()),
        Some(Value::Uint8Array(view)) => Ok(view.buffer.bytes.borrow()
            [view.byte_offset..view.byte_offset + view.length]
            .to_vec()),
        _ => Err(VmError::EvalError("expected string or bytes".into())),
    }
}

fn buffer_byte_length(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(
        string_or_bytes(arguments.first())?.len() as f64
    ))
}

fn require_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::EvalError("require expects a module name".into()));
    };
    if name != "node:path" && name != "path" {
        if name == "assert"
            || name == "node:assert"
            || name == "assert/strict"
            || name == "node:assert/strict"
        {
            let module = assert_module();
            return if name.ends_with("/strict") {
                Ok(quench_runtime::execute::set_property(
                    module.clone(),
                    "strict",
                    module,
                ))
            } else {
                Ok(module)
            };
        }
        if name == "process" || name == "node:process" {
            return Ok(process_module());
        }
        if name == "buffer" || name == "node:buffer" {
            return Ok(Value::object(vec![("Buffer".into(), buffer_module())]));
        }
        if name == "node:fs" || name == "fs" {
            return Ok(Value::object(vec![(
                "readFileSync".into(),
                capability_function(HostCapabilityKind::Custom(7)),
            )]));
        }
        if name == "node:crypto" || name == "crypto" {
            return Ok(Value::object(vec![(
                "createHash".into(),
                capability_function(HostCapabilityKind::Custom(8)),
            )]));
        }
        if name == "node:stream" || name == "stream" {
            return Ok(Value::object(vec![(
                "Transform".into(),
                capability_function(HostCapabilityKind::Custom(10)),
            )]));
        }
        if name == "node:http" || name == "http" {
            return Ok(Value::object(vec![
                (
                    "createServer".into(),
                    capability_function(HostCapabilityKind::Custom(11)),
                ),
                (
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(12)),
                ),
            ]));
        }
        if name == "url" || name == "node:url" {
            return Ok(quench_runtime::host_api::object(vec![(
                "URL".into(),
                capability_function(HostCapabilityKind::Custom(40)),
            )]));
        }
        if name == "util" || name == "node:util" {
            return Ok(util_module());
        }
        if name == "os" || name == "node:os" {
            return Ok(os_module());
        }
        return Err(VmError::EvalError(format!("Cannot find module '{name}'")));
    }
    let basename = capability_function(HostCapabilityKind::Custom(2));
    Ok(Value::object(vec![("basename".into(), basename)]))
}

fn assert_module() -> Value {
    let mut module = capability_function(HostCapabilityKind::Custom(13));
    for (name, id) in [
        ("strictEqual", 14),
        ("deepStrictEqual", 15),
        ("ok", 16),
        ("throws", 17),
        ("doesNotThrow", 18),
        ("ifError", 19),
        ("match", 20),
        ("notStrictEqual", 24),
        ("notDeepStrictEqual", 25),
        ("AssertionError", 26),
    ] {
        module = quench_runtime::execute::set_property(
            module,
            name,
            capability_function(HostCapabilityKind::Custom(id)),
        );
    }
    module
}

fn process_module() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "argv".into(),
            quench_runtime::host_api::array(std::env::args().map(Value::String).collect()),
        ),
        (
            "execPath".into(),
            Value::String(std::env::args().next().unwrap_or_default()),
        ),
        ("argv0".into(), Value::String("node".into())),
        (
            "cwd".into(),
            capability_function(HostCapabilityKind::Custom(6)),
        ),
        (
            "nextTick".into(),
            capability_function(HostCapabilityKind::Custom(21)),
        ),
    ])
}

fn url_object(url: &url::Url, id: u16) -> Value {
    let string = url.to_string();
    quench_runtime::host_api::object(vec![
        ("href".into(), Value::String(string.clone())),
        (
            "origin".into(),
            Value::String(url.origin().ascii_serialization()),
        ),
        (
            "protocol".into(),
            Value::String(url.scheme().to_string() + ":"),
        ),
        ("username".into(), Value::String(url.username().into())),
        (
            "password".into(),
            Value::String(url.password().unwrap_or("").into()),
        ),
        (
            "hostname".into(),
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "host".into(),
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "port".into(),
            Value::String(url.port().map(|port| port.to_string()).unwrap_or_default()),
        ),
        ("pathname".into(), Value::String(url.path().into())),
        (
            "search".into(),
            Value::String(
                url.query()
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default(),
            ),
        ),
        (
            "hash".into(),
            Value::String(
                url.fragment()
                    .map(|fragment| format!("#{fragment}"))
                    .unwrap_or_default(),
            ),
        ),
        (
            "toString".into(),
            capability_function(HostCapabilityKind::Custom(id)),
        ),
        (
            "toJSON".into(),
            capability_function(HostCapabilityKind::Custom(id)),
        ),
    ])
}

fn buffer_module() -> Value {
    let mut buffer = capability_function(HostCapabilityKind::Custom(30));
    for (name, kind) in [
        ("from", HostCapabilityKind::Custom(30)),
        ("alloc", HostCapabilityKind::Custom(31)),
        ("isBuffer", HostCapabilityKind::Custom(32)),
        ("byteLength", HostCapabilityKind::Custom(9)),
    ] {
        buffer = quench_runtime::execute::set_property(buffer, name, capability_function(kind));
    }
    buffer
}

fn buffer_from(arguments: &[Value]) -> Result<Value, VmError> {
    match arguments.first() {
        Some(Value::String(value)) => Ok(quench_runtime::host_api::bytes(value.as_bytes())),
        Some(Value::Uint8Array(view)) => Ok(Value::Uint8Array(view.clone())),
        _ => Err(VmError::EvalError(
            "Buffer.from expects bytes or a string".into(),
        )),
    }
}

fn buffer_alloc(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(length)) = arguments.first() else {
        return Err(VmError::EvalError("Buffer.alloc expects a length".into()));
    };
    if !length.is_finite() || *length < 0.0 {
        return Err(VmError::EvalError("invalid buffer length".into()));
    }
    Ok(quench_runtime::host_api::bytes(&vec![0; *length as usize]))
}

fn buffer_is_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(matches!(
        arguments.first(),
        Some(Value::Uint8Array(_))
    )))
}

fn util_module() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "format".into(),
            capability_function(HostCapabilityKind::Custom(80)),
        ),
        (
            "inspect".into(),
            capability_function(HostCapabilityKind::Custom(81)),
        ),
        ("types".into(), quench_runtime::host_api::object(vec![])),
    ])
}

fn util_format(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        arguments
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
    ))
}

fn util_inspect(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        arguments
            .first()
            .map(value_to_string)
            .unwrap_or_else(|| "undefined".into()),
    ))
}

fn os_module() -> Value {
    quench_runtime::host_api::object(vec![
        (
            "platform".into(),
            capability_function(HostCapabilityKind::Custom(82)),
        ),
        (
            "arch".into(),
            capability_function(HostCapabilityKind::Custom(83)),
        ),
        (
            "tmpdir".into(),
            capability_function(HostCapabilityKind::Custom(84)),
        ),
        (
            "homedir".into(),
            capability_function(HostCapabilityKind::Custom(85)),
        ),
        ("EOL".into(), Value::String("\n".into())),
    ])
}

fn os_value(kind: HostCapabilityKind) -> Result<Value, VmError> {
    match kind {
        HostCapabilityKind::Custom(82) => Ok(Value::String(std::env::consts::OS.into())),
        HostCapabilityKind::Custom(83) => Ok(Value::String(std::env::consts::ARCH.into())),
        HostCapabilityKind::Custom(85) => Ok(Value::String(
            std::env::var("HOME").unwrap_or_else(|_| "/".into()),
        )),
        _ => Ok(Value::String(
            std::env::temp_dir().to_string_lossy().into_owned(),
        )),
    }
}

fn assertion_call(id: u16, arguments: &[Value]) -> Result<Value, VmError> {
    let failed = |message: &str| Err(VmError::EvalError(format!("AssertionError: {message}")));
    match id {
        13 | 16 => {
            if arguments.first().is_some_and(is_truthy) {
                Ok(Value::Undefined)
            } else {
                failed("expected a truthy value")
            }
        }
        14 | 15 => {
            if arguments.first() == arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        17 => {
            let Some(callback) = arguments.first() else {
                return failed("missing callback");
            };
            match quench_runtime::execute::call(callback, &Value::Undefined, &[]) {
                Ok(_) => failed("expected an exception"),
                Err(_) => Ok(Value::Undefined),
            }
        }
        18 => {
            let Some(callback) = arguments.first() else {
                return failed("missing callback");
            };
            match quench_runtime::execute::call(callback, &Value::Undefined, &[]) {
                Ok(_) => Ok(Value::Undefined),
                Err(error) => Err(error),
            }
        }
        19 => {
            if matches!(arguments.first(), Some(Value::Null | Value::Undefined)) {
                Ok(Value::Undefined)
            } else {
                failed("unexpected error")
            }
        }
        20 => Ok(Value::Undefined),
        24 => {
            if arguments.first() != arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are equal")
            }
        }
        25 => {
            if arguments.first() != arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are deeply equal")
            }
        }
        _ => Err(VmError::NotCallable),
    }
}

fn next_tick(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = arguments.first() else {
        return Err(VmError::EvalError("nextTick expects a callback".into()));
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &arguments[1..])
        .map(|_| Value::Undefined)
}

fn timer_call(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = arguments.first() else {
        return Err(VmError::EvalError("timer expects a callback".into()));
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &[]).map(|_| Value::Number(1.0))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null | Value::Boolean(false) => false,
        Value::Number(number) => *number != 0.0 && !number.is_nan(),
        _ => true,
    }
}

fn capability_function(kind: HostCapabilityKind) -> Value {
    quench_runtime::host_api::capability_function(HostCapabilityRef {
        realm: RealmId::ROOT,
        kind,
    })
}

fn basename(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("path.basename expects a string".into()));
    };
    Ok(Value::String(
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .into(),
    ))
}

impl JsRuntime for QuenchRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let program =
            match path.is_some_and(|path| path.extension().is_some_and(|ext| ext == "mjs")) {
                true => quench_runtime::reduce::reduce_module_source(source),
                false => quench_runtime::reduce::reduce_source(source),
            }
            .map_err(|errors| errors.join("\n"))?;
        let capability = HostCapabilityRef {
            realm: RealmId::ROOT,
            kind: HostCapabilityKind::Custom(1),
        };
        let context = VmContext::for_realm(
            RealmId::ROOT,
            vec![
                HostCapabilityKind::Custom(1),
                HostCapabilityKind::Custom(2),
                HostCapabilityKind::Custom(3),
                HostCapabilityKind::Custom(4),
                HostCapabilityKind::Custom(5),
                HostCapabilityKind::Custom(6),
                HostCapabilityKind::Custom(7),
                HostCapabilityKind::Custom(8),
                HostCapabilityKind::Custom(9),
                HostCapabilityKind::Custom(10),
            ],
        )
        .with_host(Rc::new(QuenchNodeHost::default()))
        .with_host_capability("require", capability)
        .with_host_capability(
            "console",
            HostCapabilityRef {
                realm: RealmId::ROOT,
                kind: HostCapabilityKind::Custom(3),
            },
        )
        .with_host_value("process", process_module())
        .with_host_value("URL", capability_function(HostCapabilityKind::Custom(40)))
        .with_host_value(
            "setImmediate",
            capability_function(HostCapabilityKind::Custom(27)),
        )
        .with_host_value(
            "setTimeout",
            capability_function(HostCapabilityKind::Custom(28)),
        )
        .with_host_value(
            "setInterval",
            capability_function(HostCapabilityKind::Custom(28)),
        )
        .with_host_value(
            "clearImmediate",
            capability_function(HostCapabilityKind::Custom(29)),
        )
        .with_host_value("Buffer", buffer_module());
        quench_runtime::execute::execute_with_context(program.ops(), &context)
            .map(|_| ())
            .map_err(|error| error.render().into())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn has_pending_jobs(&self) -> bool {
        false
    }
}

pub(crate) struct QuickJsRuntime {
    runtime: rquickjs::Runtime,
}

impl QuickJsRuntime {
    pub(crate) fn new() -> Result<Self, rquickjs::Error> {
        Ok(Self {
            runtime: rquickjs::Runtime::new()?,
        })
    }
}

impl JsRuntime for QuickJsRuntime {
    fn execute(
        &self,
        source: &str,
        path: Option<&Path>,
        _host: &dyn NodeHost,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::quickjs_backend::execute_source(source, &self.runtime, path)?;
        while self.has_pending_jobs() {
            self.poll_jobs()?;
        }
        Ok(())
    }

    fn poll_jobs(&self) -> Result<bool, Box<dyn std::error::Error>> {
        self.runtime
            .execute_pending_job()
            .map_err(|error| format!("QuickJS job failed: {error:?}").into())
    }

    fn has_pending_jobs(&self) -> bool {
        self.runtime.is_job_pending()
    }
}
