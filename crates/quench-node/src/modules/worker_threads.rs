//! Rust-owned `worker_threads` boundary.
//!
//! Message channels reuse the canonical EventTarget registry. Worker launch is
//! a bounded host operation: a child `run` process receives facts through the
//! environment and returns message markers on stdout.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const WORKER_START: u16 = 2475;
const WORKER_MESSAGE: u16 = 2476;
const WORKER_CLOSE: u16 = 2477;
const WORKER_CONSTRUCT: u16 = 2478;
const MESSAGE_PORT_CONSTRUCT: u16 = 2479;
const MESSAGE_PORT_CALL: u16 = 2480;
const RECEIVE_MESSAGE: u16 = 2481;
const SET_ENVIRONMENT: u16 = 2482;
const GET_ENVIRONMENT: u16 = 2483;
const WORKER_NOOP: u16 = 2484;
const WORKER_REF: u16 = 2485;
const WORKER_UNREF: u16 = 2486;
const WORKER_HAS_REF: u16 = 2487;
const WORKER_TERMINATE: u16 = 2488;
const WORKER_COMPLETE: u16 = 2489;
const WORKER_BOOT_MESSAGE: u16 = 2490;
const BROADCAST_CHANNEL: u16 = 2508;
const BROADCAST_CHANNEL_CLOSE: u16 = 2509;
const BROADCAST_CHANNEL_INSPECT: u16 = 2510;
const BROADCAST_CHANNEL_ID: &str = "\0quench:broadcast-channel";

thread_local! {
    static ENVIRONMENT_DATA: RefCell<Vec<(String, Value)>> = const { RefCell::new(Vec::new()) };
    static WORKER_FLAGS: RefCell<HashMap<u64, (bool, bool)>> = RefCell::new(HashMap::new());
}

fn cap(kind: u16) -> Value {
    let spec = match kind {
        WORKER_START => crate::registry::SPEC_WORKER_START,
        WORKER_MESSAGE => crate::registry::SPEC_WORKER_MESSAGE,
        WORKER_CLOSE => crate::registry::SPEC_WORKER_CLOSE,
        WORKER_CONSTRUCT => crate::registry::SPEC_WORKER_CONSTRUCT,
        MESSAGE_PORT_CONSTRUCT => crate::registry::SPEC_MESSAGE_PORT_CONSTRUCT,
        MESSAGE_PORT_CALL => crate::registry::SPEC_MESSAGE_PORT_CALL,
        RECEIVE_MESSAGE => crate::registry::SPEC_WORKER_RECEIVE_MESSAGE,
        SET_ENVIRONMENT => crate::registry::SPEC_WORKER_SET_ENVIRONMENT,
        GET_ENVIRONMENT => crate::registry::SPEC_WORKER_GET_ENVIRONMENT,
        WORKER_NOOP => crate::registry::SPEC_WORKER_NOOP,
        WORKER_REF => crate::registry::SPEC_WORKER_REF,
        WORKER_UNREF => crate::registry::SPEC_WORKER_UNREF,
        WORKER_HAS_REF => crate::registry::SPEC_WORKER_HAS_REF,
        WORKER_TERMINATE => crate::registry::SPEC_WORKER_TERMINATE,
        WORKER_COMPLETE => crate::registry::SPEC_WORKER_COMPLETE,
        BROADCAST_CHANNEL => crate::registry::SPEC_BROADCAST_CHANNEL,
        BROADCAST_CHANNEL_CLOSE => crate::registry::SPEC_BROADCAST_CHANNEL_CLOSE,
        BROADCAST_CHANNEL_INSPECT => crate::registry::SPEC_BROADCAST_CHANNEL_INSPECT,
        0x0145 => crate::registry::SPEC_MESSAGE_CHANNEL,
        _ => crate::registry::NodeSpec::new("worker_threads:internal", kind),
    };
    crate::host::capability(spec)
}

fn worker_mode() -> bool {
    std::env::var("QUENCH_WORKER").ok().as_deref() == Some("1")
        || std::env::args().any(|arg| arg == "--quench-worker")
}

pub fn build(state: &Rc<std::cell::RefCell<HostState>>) -> Result<Value, VmError> {
    let main = !worker_mode();
    let parent = if main {
        Value::Null
    } else {
        parent_port(state)?
    };
    let worker_data = if main {
        Value::Undefined
    } else {
        std::env::var("QUENCH_WORKER_DATA")
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .map(from_json)
            .unwrap_or(Value::Null)
    };
    if !main {
        if let Ok(message) = std::env::var("QUENCH_WORKER_MESSAGE") {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) {
                let event = from_json(value);
                state
                    .borrow_mut()
                    .event_loop
                    .queue_microtask(cap(WORKER_BOOT_MESSAGE), vec![parent.clone(), event]);
            }
        }
    }
    let message_channel = cap(crate::registry::SPEC_MESSAGE_CHANNEL.cap);
    let message_port = cap(MESSAGE_PORT_CALL);
    let _ =
        execute::set_callable_property(&message_port, "name", Value::String("MessagePort".into()));
    let message_port_prototype = host_api::object(vec![
        ("constructor".into(), message_port.clone()),
        (
            "postMessage".into(),
            crate::host::capability(crate::registry::SPEC_MESSAGE_PORT_POST),
        ),
        (
            "close".into(),
            crate::host::capability(crate::registry::SPEC_MESSAGE_PORT_CLOSE),
        ),
        (
            "start".into(),
            crate::host::capability(crate::registry::SPEC_MESSAGE_PORT_START),
        ),
        (
            "ref".into(),
            crate::host::capability(crate::registry::SPEC_MESSAGE_PORT_REF),
        ),
        (
            "unref".into(),
            crate::host::capability(crate::registry::SPEC_MESSAGE_PORT_UNREF),
        ),
        (
            "hasRef".into(),
            crate::host::capability(crate::registry::SPEC_MESSAGE_PORT_HAS_REF),
        ),
        ("onmessage".into(), Value::Null),
        ("onmessageerror".into(), Value::Null),
    ]);
    let global = quench_runtime::vm::current_global_object();
    let event_target = execute::get_property(&global, "EventTarget");
    let event_target_prototype = execute::get_property(&event_target, "prototype");
    let message_port_prototype =
        execute::set_prototype_of(&message_port_prototype, &event_target_prototype)
            .unwrap_or(message_port_prototype);
    let _ =
        execute::set_callable_property(&message_port, "prototype", message_port_prototype.clone());
    crate::modules::event_target::set_message_port_prototype(message_port_prototype);
    let worker = cap(WORKER_CONSTRUCT);
    let broadcast_channel = cap(BROADCAST_CHANNEL);
    Ok(host_api::object(vec![
        ("isMainThread".into(), Value::Boolean(main)),
        (
            "threadId".into(),
            Value::Number(if main { 0.0 } else { 1.0 }),
        ),
        ("workerData".into(), worker_data),
        ("parentPort".into(), parent),
        ("MessageChannel".into(), message_channel),
        ("MessagePort".into(), message_port),
        ("BroadcastChannel".into(), broadcast_channel),
        ("Worker".into(), worker),
        ("receiveMessageOnPort".into(), cap(RECEIVE_MESSAGE)),
        ("SHARE_ENV".into(), host_api::object(Vec::new())),
        ("markAsUncloneable".into(), cap(WORKER_NOOP)),
        ("markAsUntransferable".into(), cap(WORKER_NOOP)),
        ("setEnvironmentData".into(), cap(SET_ENVIRONMENT)),
        ("getEnvironmentData".into(), cap(GET_ENVIRONMENT)),
    ]))
}

fn parent_port(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    let port = crate::modules::events::new_emitter_object(state)?;
    execute::set_property_in_place(&port, "postMessage", cap(WORKER_MESSAGE));
    execute::set_property_in_place(&port, "close", cap(WORKER_CLOSE));
    Ok(port)
}

macro_rules! handlers { ($(($name:ident, $kind:ident)),+ $(,)?) => { $(
    pub fn $name(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
        call($kind, state, receiver, args)
    }
)+ }; }
handlers! {
    (worker_start_handler, WORKER_START), (worker_message_handler, WORKER_MESSAGE),
    (worker_close_handler, WORKER_CLOSE),
    (message_port_call_handler, MESSAGE_PORT_CALL), (receive_message_handler, RECEIVE_MESSAGE),
    (set_environment_handler, SET_ENVIRONMENT), (get_environment_handler, GET_ENVIRONMENT),
    (worker_ref_handler, WORKER_REF), (worker_unref_handler, WORKER_UNREF),
    (worker_has_ref_handler, WORKER_HAS_REF), (worker_terminate_handler, WORKER_TERMINATE),
    (worker_complete_handler, WORKER_COMPLETE), (worker_noop_handler, WORKER_NOOP),
    (worker_boot_message_handler, WORKER_BOOT_MESSAGE),
    (broadcast_channel_handler, BROADCAST_CHANNEL),
    (broadcast_channel_close_handler, BROADCAST_CHANNEL_CLOSE),
    (broadcast_channel_inspect_handler, BROADCAST_CHANNEL_INSPECT),
}

pub fn message_port_construct(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::event_target::new_message_port(state)
}

pub fn message_port_construct_handler(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    message_port_construct(state, _args)
}

pub fn worker_construct_handler(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    worker_new(state, args)
}

pub fn broadcast_channel_construct_handler(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    broadcast_channel_new(state, args)
}

fn broadcast_channel_new(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = match args.first() {
        Some(Value::String(name)) => name.clone(),
        Some(_) => return Err(type_error("The \"name\" argument must be a string")),
        None => String::new(),
    };
    let object = host_api::object(vec![
        (BROADCAST_CHANNEL_ID.into(), Value::Boolean(true)),
        ("name".into(), Value::String(name)),
        ("active".into(), Value::Boolean(true)),
    ]);
    let descriptor = |value| {
        host_api::object(vec![
            ("value".into(), value),
            ("writable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ])
    };
    let object = execute::define_property(object, "close", descriptor(cap(BROADCAST_CHANNEL_CLOSE)))?;
    let object = execute::define_property(
        object,
        "Symbol.for.nodejs.util.inspect.custom\0",
        descriptor(cap(BROADCAST_CHANNEL_INSPECT)),
    )?;
    Ok(object)
}

fn broadcast_channel_receiver(receiver: Option<&Value>) -> Result<&Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(invalid_this());
    };
    if !matches!(
        execute::get_property(receiver, BROADCAST_CHANNEL_ID),
        Value::Boolean(true)
    ) {
        return Err(invalid_this());
    }
    Ok(receiver)
}

fn broadcast_channel_close(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = broadcast_channel_receiver(receiver)?;
    execute::set_property_in_place(receiver, "active", Value::Boolean(false));
    Ok(Value::Undefined)
}

fn broadcast_channel_inspect(
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = broadcast_channel_receiver(receiver)?;
    let depth = args.first().and_then(|value| match value {
        Value::Number(depth) => Some(*depth),
        _ => None,
    });
    if depth.is_some_and(|depth| depth < 0.0) {
        return Ok(Value::String("BroadcastChannel".into()));
    }
    let name = execute::get_property(receiver, "name");
    let active = execute::get_property(receiver, "active");
    let name = execute::to_js_string(&name).unwrap_or_default();
    let active = matches!(active, Value::Boolean(true));
    Ok(Value::String(format!(
        "BroadcastChannel {{ name: '{}', active: {active} }}",
        name.replace('\\', "\\\\").replace('\'', "\\'")
    )))
}

fn invalid_this() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_THIS".into())),
        (
            "message".into(),
            Value::String("Value of \"this\" must be of type BroadcastChannel".into()),
        ),
    ]))
}

fn call(
    kind: u16,
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match kind {
        WORKER_CONSTRUCT => worker_new(state, args),
        WORKER_START => worker_start(state, args),
        WORKER_MESSAGE => worker_post_message(state, receiver, args),
        WORKER_CLOSE => worker_close(state, receiver, args),
        WORKER_REF => worker_ref(state, receiver),
        WORKER_UNREF => worker_unref(state, receiver),
        WORKER_HAS_REF => Ok(worker_has_ref(receiver)),
        WORKER_TERMINATE => worker_terminate(state, receiver),
        WORKER_COMPLETE => worker_complete(state, args),
        WORKER_BOOT_MESSAGE => {
            if let Some(port) = args.first() {
                let _ = crate::modules::events::method_emit(
                    state,
                    Some(port),
                    &[
                        Value::String("message".into()),
                        args.get(1).cloned().unwrap_or(Value::Undefined),
                    ],
                );
            }
            Ok(Value::Undefined)
        }
        BROADCAST_CHANNEL => broadcast_channel_new(state, args),
        BROADCAST_CHANNEL_CLOSE => broadcast_channel_close(receiver),
        BROADCAST_CHANNEL_INSPECT => broadcast_channel_inspect(receiver, args),
        WORKER_NOOP => Ok(args.first().cloned().unwrap_or(Value::Undefined)),
        MESSAGE_PORT_CALL | MESSAGE_PORT_CONSTRUCT => message_port_construct(state, args),
        RECEIVE_MESSAGE => receive_message(state, args),
        SET_ENVIRONMENT => set_environment(args),
        GET_ENVIRONMENT => get_environment(args),
        _ => Ok(Value::Undefined),
    }
}

fn worker_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(
        filename,
        Value::String(_) | Value::Object(_) | Value::ObjectAlias(_)
    ) {
        return Err(type_error("The \"filename\" argument must be a string"));
    }
    let options = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    if let Ok(env) = execute::get_property_result(&options, "env") {
        if !matches!(
            env,
            Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(type_error(
                "The \"options.env\" property must be of type object",
            ));
        }
    }
    let data = execute::get_property(&options, "workerData");
    let transfer_list = execute::get_property(&options, "transferList");
    if contains_port(&data, state) && !transfer_contains(&transfer_list, state, &data) {
        return Err(quench_runtime::execute::VmError::Thrown(
            quench_runtime::builtins::dom_exception(
                "Object that needs transfer was found in message but not listed in transferList",
                "DataCloneError",
            ),
        ));
    }
    if let Value::Array(list) = transfer_list {
        let length = match execute::get_property(&Value::Array(list.clone()), "length") {
            Value::Number(number) if number.is_finite() && number >= 0.0 => number as usize,
            _ => 0,
        };
        for index in 0..length {
            if let Value::ArrayBuffer(buffer) =
                execute::get_property(&Value::Array(list.clone()), &index.to_string())
            {
                if buffer.untransferable {
                    return Err(type_error("Cannot transfer object of an unsupported type"));
                }
                buffer.detach();
            }
        }
    }
    let worker = crate::modules::events::new_emitter_object(state)?;
    execute::set_property_in_place(
        &worker,
        "\0worker-state",
        host_api::object(vec![
            ("refed".into(), Value::Boolean(true)),
            ("destroyed".into(), Value::Boolean(false)),
        ]),
    );
    let worker = crate::modules::async_hooks::worker_resource(state, None, &[worker])?;
    if let Some(id) = worker_id(&worker) {
        WORKER_FLAGS.with(|flags| {
            flags.borrow_mut().insert(id, (true, false));
        });
    }
    for (name, value) in [
        ("threadId", Value::Number(1.0)),
        ("exited", Value::Boolean(false)),
        ("_worker-filename", filename),
        ("_worker-options", options),
        ("_worker-refed", Value::Boolean(true)),
        ("_worker-started", Value::Boolean(false)),
        ("_worker-destroyed", Value::Boolean(false)),
    ] {
        execute::set_property_in_place(&worker, name, value);
    }
    let stdout = crate::modules::events::new_emitter_object(state)?;
    let stderr = crate::modules::events::new_emitter_object(state)?;
    execute::set_property_in_place(&worker, "stdout", stdout);
    execute::set_property_in_place(&worker, "stderr", stderr);
    execute::set_property_in_place(
        &execute::get_property(&worker, "stdout"),
        "setEncoding",
        cap(WORKER_NOOP),
    );
    execute::set_property_in_place(
        &execute::get_property(&worker, "stderr"),
        "setEncoding",
        cap(WORKER_NOOP),
    );
    execute::set_property_in_place(&worker, "postMessage", cap(WORKER_START));
    execute::set_property_in_place(&worker, "ref", cap(WORKER_REF));
    execute::set_property_in_place(&worker, "unref", cap(WORKER_UNREF));
    execute::set_property_in_place(&worker, "hasRef", cap(WORKER_HAS_REF));
    execute::set_property_in_place(&worker, "terminate", cap(WORKER_TERMINATE));
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(cap(WORKER_START), vec![worker.clone(), Value::Undefined]);
    Ok(worker)
}

fn contains_port(value: &Value, state: &Rc<RefCell<HostState>>) -> bool {
    if crate::modules::event_target::is_message_port(state, value) {
        return true;
    }
    match value {
        Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_) => {
            execute::own_enumerable_keys(value)
                .into_iter()
                .any(|key| contains_port(&execute::get_property(value, &key), state))
        }
        _ => false,
    }
}

fn transfer_contains(transfer: &Value, state: &Rc<RefCell<HostState>>, value: &Value) -> bool {
    let listed = if let Value::Array(_) = transfer {
        let length = match execute::get_property(transfer, "length") {
            Value::Number(number) if number.is_finite() && number >= 0.0 => number as usize,
            _ => 0,
        };
        (0..length)
            .filter_map(|index| {
                let item = execute::get_property(transfer, &index.to_string());
                crate::modules::event_target::is_message_port(state, &item)
                    .then(|| crate::modules::event_target::target_identity(&item))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    fn walk(value: &Value, state: &Rc<RefCell<HostState>>, listed: &[Option<u64>]) -> bool {
        if crate::modules::event_target::is_message_port(state, value) {
            return listed.contains(&crate::modules::event_target::target_identity(value));
        }
        match value {
            Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_) => {
                execute::own_enumerable_keys(value)
                    .into_iter()
                    .all(|key| walk(&execute::get_property(value, &key), state, listed))
            }
            _ => true,
        }
    }
    walk(value, state, &listed)
}

fn worker_start(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(worker) = args.first() else {
        return Ok(Value::Undefined);
    };
    if matches!(
        execute::get_property(worker, "_worker-started"),
        Value::Boolean(true)
    ) {
        return Ok(worker.clone());
    }
    if !worker_is_refed(worker) {
        return Ok(worker.clone());
    }
    execute::set_property_in_place(worker, "_worker-started", Value::Boolean(true));
    let filename = execute::get_property(worker, "_worker-filename");
    let options = execute::get_property(worker, "_worker-options");
    let message = args.get(1).cloned().unwrap_or(Value::Undefined);
    let output = launch(filename, options, message, state)?;
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(cap(WORKER_COMPLETE), vec![worker.clone(), output]);
    Ok(worker.clone())
}

fn worker_is_refed(worker: &Value) -> bool {
    let Some(id) = worker_id(worker) else {
        return !matches!(execute::get_property(worker, "_worker-refed"), Value::Boolean(false));
    };
    WORKER_FLAGS.with(|flags| flags.borrow().get(&id).map(|entry| entry.0).unwrap_or(true))
}

fn launch(
    filename: Value,
    options: Value,
    message: Value,
    state: &Rc<RefCell<HostState>>,
) -> Result<Value, VmError> {
    let filename = execute::to_js_string(&filename).unwrap_or_default();
    let eval = matches!(
        execute::get_property(&options, "eval"),
        Value::Boolean(true)
    );
    let data = execute::get_property(&options, "workerData");
    let encoded = serde_json::to_string(&to_json(&data)).unwrap_or_else(|_| "null".into());
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("run")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| std::path::PathBuf::from(&state.borrow().process.exec_path));
    let mut command = std::process::Command::new(executable);
    let mut temp = None;
    if eval {
        let path = std::env::temp_dir().join(format!(
            "quench-worker-{}-{}.js",
            std::process::id(),
            unique_id()
        ));
        let source = format!("globalThis.__quench_worker_mode = true;\n{filename}\n");
        std::fs::write(&path, source).map_err(|error| VmError::EvalError(error.to_string()))?;
        command.arg(&path);
        temp = Some(path);
    } else {
        command.arg(&filename);
    }
    command.args([
        "--quench-worker",
        &format!("--quench-worker-data={encoded}"),
    ]);
    command
        .env("QUENCH_WORKER", "1")
        .env("QUENCH_CHILD_RUNNER", "1");
    if !matches!(message, Value::Undefined) {
        command.env(
            "QUENCH_WORKER_MESSAGE",
            serde_json::to_string(&to_json(&message)).unwrap_or_else(|_| "null".into()),
        );
    }
    let exec_argv = execute::get_property(&options, "execArgv");
    if !matches!(exec_argv, Value::Undefined) {
        command.env(
            "QUENCH_EXEC_ARGV",
            serde_json::to_string(&to_json(&exec_argv)).unwrap_or_else(|_| "[]".into()),
        );
    }
    if let Value::String(cwd) = execute::get_property(&options, "cwd") {
        command.current_dir(cwd);
    }
    let result = command
        .output()
        .map_err(|error| VmError::EvalError(error.to_string()));
    if let Some(path) = temp {
        let _ = std::fs::remove_file(path);
    }
    let output = result?;
    Ok(host_api::object(vec![
        (
            "status".into(),
            Value::Number(output.status.code().unwrap_or(1) as f64),
        ),
        (
            "stdout".into(),
            Value::String(String::from_utf8_lossy(&output.stdout).into_owned()),
        ),
        (
            "stderr".into(),
            Value::String(String::from_utf8_lossy(&output.stderr).into_owned()),
        ),
    ]))
}

fn unique_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0)
}

fn worker_complete(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(worker) = args.first() else {
        return Ok(Value::Undefined);
    };
    let output = args.get(1).cloned().unwrap_or(Value::Undefined);
    let shared = execute::get_property(worker, "\0worker-state");
    execute::set_property_in_place(&shared, "destroyed", Value::Boolean(false));
    let stdout = execute::get_property(&output, "stdout");
    let stderr = execute::get_property(&output, "stderr");
    let stdout_obj = execute::get_property(worker, "stdout");
    let stderr_obj = execute::get_property(worker, "stderr");
    if let Value::String(text) = stdout {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&stdout_obj),
            &[
                Value::String("data".into()),
                crate::modules::buffer_proto::make_buffer(text.as_bytes()),
            ],
        );
        parse_messages(state, worker, &text);
    }
    if let Value::String(text) = stderr {
        let _ = crate::modules::events::method_emit(
            state,
            Some(&stderr_obj),
            &[
                Value::String("data".into()),
                crate::modules::buffer_proto::make_buffer(text.as_bytes()),
            ],
        );
    }
    let _ = crate::modules::events::method_emit(
        state,
        Some(&stdout_obj),
        &[Value::String("end".into())],
    );
    let _ = crate::modules::events::method_emit(
        state,
        Some(&stderr_obj),
        &[Value::String("end".into())],
    );
    let status = execute::get_property(&output, "status");
    let _ =
        crate::modules::events::method_emit(state, Some(worker), &[Value::String("online".into())]);
    let _ = crate::modules::events::method_emit(
        state,
        Some(worker),
        &[Value::String("exit".into()), status.clone()],
    );
    execute::set_property_in_place(worker, "exited", Value::Boolean(true));
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(cap(WORKER_CLOSE), vec![worker.clone()]);
    Ok(Value::Undefined)
}

fn parse_messages(state: &Rc<RefCell<HostState>>, worker: &Value, text: &str) {
    for line in text
        .lines()
        .filter_map(|line| line.strip_prefix("__QUENCH_WORKER_MESSAGE__"))
    {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            let _ = crate::modules::events::method_emit(
                state,
                Some(worker),
                &[Value::String("message".into()), from_json(json)],
            );
        }
    }
}

fn worker_post_message(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(port) = receiver else {
        return Err(type_error("Cannot post message without a parentPort"));
    };
    if execute::get_property(port, "_worker-filename") != Value::Undefined {
        return worker_start(
            _state,
            &[
                port.clone(),
                args.first().cloned().unwrap_or(Value::Undefined),
            ],
        );
    }
    if !quench_runtime::is_callable(&execute::get_property(port, "emit")) {
        return Ok(Value::Undefined);
    }
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let encoded = serde_json::to_string(&to_json(&value)).unwrap_or_else(|_| "null".into());
    use std::io::Write;
    let _ = std::io::stdout().write_all(format!("__QUENCH_WORKER_MESSAGE__{encoded}\n").as_bytes());
    Ok(Value::Undefined)
}

fn worker_close(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let worker = receiver
        .filter(|value| execute::get_property(value, "_worker-filename") != Value::Undefined)
        .or_else(|| args.first());
    if let Some(worker) = worker {
        execute::set_property_in_place(worker, "_worker-destroyed", Value::Boolean(true));
        let shared = execute::get_property(worker, "\0worker-state");
        execute::set_property_in_place(&shared, "destroyed", Value::Boolean(true));
    }
    if let Some(id) = worker.and_then(worker_id) {
        WORKER_FLAGS.with(|flags| {
            if let Some(entry) = flags.borrow_mut().get_mut(&id) {
                entry.1 = true;
            }
        });
    }
    Ok(worker.cloned().unwrap_or(Value::Undefined))
}

fn worker_ref(_state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Result<Value, VmError> {
    if let Some(worker) = receiver {
        execute::set_property_in_place(worker, "_worker-refed", Value::Boolean(true));
        let shared = execute::get_property(worker, "\0worker-state");
        execute::set_property_in_place(&shared, "refed", Value::Boolean(true));
    }
    if let Some(id) = receiver.and_then(worker_id) {
        WORKER_FLAGS.with(|flags| {
            if let Some(entry) = flags.borrow_mut().get_mut(&id) {
                entry.0 = true;
            }
        });
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn worker_unref(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(worker) = receiver {
        execute::set_property_in_place(worker, "_worker-refed", Value::Boolean(false));
        let shared = execute::get_property(worker, "\0worker-state");
        execute::set_property_in_place(&shared, "refed", Value::Boolean(false));
    }
    if let Some(id) = receiver.and_then(worker_id) {
        WORKER_FLAGS.with(|flags| {
            if let Some(entry) = flags.borrow_mut().get_mut(&id) {
                entry.0 = false;
            }
        });
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn worker_terminate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(worker) = receiver {
        execute::set_property_in_place(worker, "_worker-destroyed", Value::Boolean(true));
    }
    // Node's terminate() is asynchronous and always returns a Promise whose
    // fulfillment value is the worker exit code.  Keep the host transition
    // synchronous, but preserve the observable promise-shaped boundary.
    Ok(Value::Promise(Rc::new(
        quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Fulfilled(Value::Number(0.0)),
        ),
    )))
}

fn worker_id(value: &Value) -> Option<u64> {
    match execute::get_property(value, "\0quench:async_hooks:id") {
        Value::Number(number) if number.is_finite() && number >= 0.0 => Some(number as u64),
        _ => None,
    }
}

fn worker_has_ref(receiver: Option<&Value>) -> Value {
    let Some(id) = receiver.and_then(worker_id) else {
        return Value::Undefined;
    };
    WORKER_FLAGS.with(|flags| match flags.borrow().get(&id) {
        Some((_refed, true)) => Value::Undefined,
        Some((refed, false)) => Value::Boolean(*refed),
        None => Value::Boolean(true),
    })
}

fn receive_message(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(port) = args.first() else {
        return Err(type_error(
            "The \"port\" argument must be a MessagePort instance",
        ));
    };
    if !crate::modules::event_target::is_message_port(state, port) {
        return Err(type_error(
            "The \"port\" argument must be a MessagePort instance",
        ));
    }
    match crate::modules::event_target::take_message(state, port) {
        Some(message) => Ok(host_api::object(vec![("message".into(), message)])),
        None => Ok(Value::Undefined),
    }
}

fn set_environment(args: &[Value]) -> Result<Value, VmError> {
    let key = execute::to_js_string(args.first().unwrap_or(&Value::Undefined)).unwrap_or_default();
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    ENVIRONMENT_DATA.with(|data| {
        let mut data = data.borrow_mut();
        if let Some(entry) = data.iter_mut().find(|(name, _)| name == &key) {
            entry.1 = value;
        } else {
            data.push((key, value));
        }
    });
    Ok(Value::Undefined)
}

fn get_environment(args: &[Value]) -> Result<Value, VmError> {
    let key = execute::to_js_string(args.first().unwrap_or(&Value::Undefined)).unwrap_or_default();
    Ok(ENVIRONMENT_DATA.with(|data| {
        data.borrow()
            .iter()
            .find(|(name, _)| name == &key)
            .map(|(_, value)| value.clone())
            .unwrap_or(Value::Undefined)
    }))
}

fn type_error(message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Undefined => serde_json::Value::Null,
        Value::Null => serde_json::Value::Null,
        Value::Boolean(value) => serde_json::Value::Bool(*value),
        Value::Number(value) => serde_json::json!(value),
        Value::String(value) | Value::BigInt(value) => serde_json::Value::String(value.clone()),
        Value::Uint8Array(array) => {
            serde_json::json!({"__quench_typed_array":"Uint8Array", "data": (0..array.logical_len()).filter_map(|index| array.get(index)).collect::<Vec<_>>() })
        }
        Value::Array(_values) => {
            let length = execute::get_property(value, "length");
            let length =
                matches!(length, Value::Number(number) if number.is_finite() && number >= 0.0)
                    .then_some(length)
                    .and_then(|value| {
                        if let Value::Number(number) = value {
                            Some(number as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
            serde_json::Value::Array(
                (0..length)
                    .map(|index| to_json(&execute::get_property(value, &index.to_string())))
                    .collect(),
            )
        }
        Value::Object(_) | Value::ObjectAlias(_) => serde_json::Value::Object(
            execute::own_enumerable_keys(value)
                .into_iter()
                .map(|key| (key.clone(), to_json(&execute::get_property(value, &key))))
                .collect(),
        ),
        _ => serde_json::Value::Null,
    }
}

fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => Value::Number(value.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(values) => {
            host_api::array(values.into_iter().map(from_json).collect())
        }
        serde_json::Value::Object(values) => {
            if values
                .get("__quench_typed_array")
                .and_then(serde_json::Value::as_str)
                == Some("Uint8Array")
            {
                let data = values
                    .get("data")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_u64)
                            .map(|value| value as u8)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                return quench_runtime::host_api::bytes(&data);
            }
            host_api::object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, from_json(value)))
                    .collect(),
            )
        }
    }
}
