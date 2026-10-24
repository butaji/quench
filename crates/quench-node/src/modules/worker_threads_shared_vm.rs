//! Genuine OS-thread execution. Each child constructs and owns its NodeHost + Vm.
//! Draft scope: eval workers, owned clone graph, owned transfer graphs and uncatchable Core termination.
use super::event_constructor_shared_vm::get;
use super::worker_error_packet::{self as error_packet, Packet};
use super::worker_owned_clone::{self as clone, Message};
use crate::host::NodeHost;
use quench_runtime::{
    Engine, ExecutionRequest, HostWeakRoot, NativeContext, RootId, RootedError, Runtime,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};
type Context<'a> = NativeContext<'a, NodeHost>;
static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);
const WORKER_FAILURE_EXIT_CODE: i32 =
    super::node_protocol_constants::ExitCode::GenericUserError as i32;
#[derive(Default)]
pub(crate) struct State {
    thread_id: u64,
    thread_name: Option<String>,
    pub(crate) exec_argv: Option<Vec<String>>,
    environment_data: HashMap<String, Message>,
    pub(crate) inherited_expose_gc: bool,
    eval_script: Option<String>,
    serialized_failure: Option<Packet>,
    child: Option<Child>,
    workers: HashMap<u64, Worker>,
    pending: VecDeque<(u64, Event)>,
    port: Option<RootId>,
}
impl State {
    pub(crate) fn captures_stderr(&self) -> bool {
        // Quench exposes a Worker.stderr emitter on every Worker, matching
        // Node's default worker stdio behavior. Route child stderr through it
        // even when the optional `stderr` flag is omitted.
        self.thread_id != 0
    }
}
struct Child {
    data: Option<Message>,
    environment_data: HashMap<String, Message>,
    rx: mpsc::Receiver<Message>,
    tx: mpsc::Sender<Event>,
    interrupt: quench_runtime::ExecutionInterrupt,
    pending_messages: VecDeque<Message>,
    broadcast_refs: usize,
    listening: bool,
    refed: bool,
    closed: bool,
}
struct Worker {
    identity: HostWeakRoot,
    root: Option<RootId>,
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Event>,
    interrupt: quench_runtime::ExecutionInterrupt,
    join: Option<thread::JoinHandle<()>>,
    alive: bool,
    cpu_tid: std::sync::Arc<AtomicU64>,
    handle_active: bool,
    async_resource: Option<RootId>,
    message_port_async_resource: Option<RootId>,
    message_port_refed: bool,
    thread_name: Option<String>,
    refed: bool,
    resolvers: Vec<RootId>,
}
struct Failure {
    exit_code: i32,
    serialized: Option<Packet>,
    name: String,
    message: String,
}

fn eval_source_kind(source: &str, exec_argv: Option<&[String]>) -> quench_runtime::SourceKind {
    let mut input_type = None;
    let mut detect_module = false;
    for argument in exec_argv.unwrap_or_default() {
        match argument.as_str() {
            "--input-type=module" => input_type = Some(quench_runtime::SourceKind::Module),
            "--input-type=commonjs" => input_type = Some(quench_runtime::SourceKind::Script),
            "--experimental-detect-module" => detect_module = true,
            "--no-experimental-detect-module" => detect_module = false,
            _ => {}
        }
    }
    if let Some(input_type) = input_type {
        return input_type;
    }
    if !detect_module {
        return quench_runtime::SourceKind::Script;
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::unambiguous()).parse();
    if parsed.program.source_type.is_module() {
        quench_runtime::SourceKind::Module
    } else {
        quench_runtime::SourceKind::Script
    }
}

fn node_options_tokens(options: &str) -> Vec<String> {
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut tokens = Vec::new();
    for character in options.chars() {
        if escaped {
            token.push(character);
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(current) = quote {
            if character == current {
                quote = None;
            } else {
                token.push(character);
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(character);
        }
    }
    if escaped {
        token.push('\\');
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn node_options_has_flag(options: &str, wanted: &str) -> bool {
    node_options_tokens(options).iter().any(|token| {
        token == wanted
            || token
                .strip_prefix(wanted)
                .is_some_and(|suffix| suffix.starts_with('='))
    })
}

impl Failure {
    fn host(message: String) -> Self {
        Self {
            exit_code: WORKER_FAILURE_EXIT_CODE,
            serialized: None,
            name: "Error".into(),
            message,
        }
    }
}
pub(crate) fn record_callback_exception(runtime: &mut Runtime<NodeHost>, root: RootId) {
    if runtime
        .host_mut()
        .shared_state()
        .borrow()
        .workers
        .child
        .is_none()
    {
        return;
    }
    if let Ok(function) =
        runtime.host_function(crate::host::shared_vm::operation("workerSerializeFailure"))
    {
        let undefined = runtime.root(quench_runtime::Value::UNDEFINED);
        let result = runtime.call_rooted(function, undefined, &[root]);
        runtime.release_root(function);
        runtime.release_root(undefined);
        match result {
            Ok(result) => {
                runtime.release_root(result);
            }
            Err(error) => {
                if let Some(exception) = error.exception {
                    runtime.release_root(exception);
                }
            }
        }
    }
}
fn completion_failure(runtime: &mut Runtime<NodeHost>, message: String) -> Failure {
    let packet = runtime
        .host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .serialized_failure
        .take();
    let control = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .clone();
    let exit_code = if control.fatal_exception_handler_failed() {
        WORKER_FAILURE_EXIT_CODE
    } else {
        control.exit_code().unwrap_or(WORKER_FAILURE_EXIT_CODE)
    };
    match packet {
        Some(packet) => Failure {
            exit_code,
            serialized: Some(packet),
            name: String::new(),
            message: String::new(),
        },
        None => {
            let mut failure = Failure::host(message);
            failure.exit_code = exit_code;
            failure
        }
    }
}

fn failure(
    runtime: &mut Runtime<NodeHost>,
    error: quench_runtime::JsError,
    callback_exception: Option<RootId>,
) -> Failure {
    if let Some(value) = error.thrown_value() {
        let root = runtime.root(value);
        record_callback_exception(runtime, root);
        runtime.release_root(root);
        if let Some(exception) = callback_exception {
            runtime.release_root(exception);
        }
    } else if let Some(exception) = callback_exception {
        // Host callbacks can retain the thrown guest value even when the
        // returned JsError carries no thrown_value. Preserve its actual
        // Error subclass before falling back to a generic host failure.
        record_callback_exception(runtime, exception);
        runtime.release_root(exception);
    } else {
        return completion_failure(runtime, error.to_string());
    }
    {
        let mut failure = completion_failure(runtime, "native error packet encoding failed".into());
        if failure.serialized.is_none() {
            failure.serialized = Some(Packet::Unsupported("native error packet encoding failed"));
        }
        failure
    }
}
enum Event {
    Online,
    Message(Message),
    InternalMessage(Message),
    Stdout(String),
    Stderr(String),
    Error(Failure),
    Exit(i32),
}
fn define(ctx: &mut Context<'_>, o: RootId, name: &str, v: RootId) -> Result<(), RootedError> {
    let k = ctx.string_rooted(name);
    if ctx.define_data_property_rooted(o, k, v, true, true, true)? {
        Ok(())
    } else {
        Err(RootedError::host("Worker property definition rejected"))
    }
}
fn define_hidden(
    ctx: &mut Context<'_>,
    o: RootId,
    name: &str,
    v: RootId,
) -> Result<(), RootedError> {
    let k = ctx.string_rooted(name);
    if ctx.define_data_property_rooted(o, k, v, true, false, true)? {
        Ok(())
    } else {
        Err(RootedError::host(
            "Worker internal property definition rejected",
        ))
    }
}
fn method(ctx: &mut Context<'_>, o: RootId, name: &str, op: &str) -> Result<(), RootedError> {
    let f = ctx.host_function_named(crate::host::shared_vm::operation(op), name)?;
    let k = ctx.string_rooted(name);
    if ctx.define_data_property_rooted(o, k, f, true, false, true)? {
        Ok(())
    } else {
        Err(RootedError::host("Worker method definition rejected"))
    }
}
fn id(ctx: &mut Context<'_>, receiver: RootId) -> Result<u64, RootedError> {
    let shared = ctx.host_mut().shared_state();
    let identities = shared
        .borrow()
        .workers
        .workers
        .iter()
        .map(|(id, w)| (*id, w.identity))
        .collect::<Vec<_>>();
    for (id, identity) in identities {
        if let Some(r) = ctx.resolve_host_weak_root(identity) {
            if ctx.strict_equal_rooted(r, receiver)? {
                return Ok(id);
            }
        }
    }
    Err(RootedError::host("Invalid Worker receiver"))
}
fn fail(ctx: &mut Context<'_>, message: &str) -> Result<RootedError, RootedError> {
    let e = ctx.error_rooted(message)?;
    super::node_errors_shared_vm::decorate_error(ctx, e, "ERR_NOT_IMPLEMENTED")?;
    Ok(ctx.throw(e))
}
fn capture_message(
    ctx: &mut Context<'_>,
    value: RootId,
    transfers: &[RootId],
) -> Result<Message, RootedError> {
    clone::encode_with_transfers(ctx, value, transfers)
        .or_else(|error| Err(super::clone_shared_vm::translate_clone_failure(ctx, error)?))
}
fn environment_data_key(ctx: &mut Context<'_>, key: RootId) -> Result<String, RootedError> {
    let value = ctx
        .rooted_value(key)
        .ok_or_else(|| RootedError::host("invalid environment data key"))?;
    if value.is_undefined() {
        return Ok("undefined".to_owned());
    }
    if value.is_null() {
        return Ok("null".to_owned());
    }
    if let Some(boolean) = value.as_bool() {
        return Ok(format!("boolean:{boolean}"));
    }
    if let Some(number) = value.as_number() {
        return Ok(format!(
            "number:{}",
            if number == 0.0 { 0.0 } else { number }
        ));
    }
    if let Some(text) = ctx.string_text(key)? {
        return Ok(format!("string:{text}"));
    }
    // Object identity is local to a VM and therefore cannot name the same
    // entry in a worker snapshot; keep its exact identity for local reads.
    Ok(format!("identity:{key:?}"))
}

pub(crate) fn set_environment_data(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key_root = arg(ctx, args, 0);
    let key = environment_data_key(ctx, key_root)?;
    let value = arg(ctx, args, 1);
    let is_undefined = ctx
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined());
    let encoded = if is_undefined {
        None
    } else {
        Some(capture_message(ctx, value, &[])?)
    };
    let shared = ctx.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    if let Some(value) = encoded {
        state.workers.environment_data.insert(key, value);
    } else {
        state.workers.environment_data.remove(&key);
    }
    drop(state);
    Ok(ctx.undefined())
}

pub(crate) fn get_environment_data(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key_root = arg(ctx, args, 0);
    let key = environment_data_key(ctx, key_root)?;
    let shared = ctx.host_mut().shared_state();
    let value = shared.borrow().workers.environment_data.get(&key).cloned();
    match value {
        Some(value) => clone::decode(ctx, &value),
        None => Ok(ctx.undefined()),
    }
}
fn arg(ctx: &mut Context<'_>, args: &[RootId], i: usize) -> RootId {
    args.get(i).copied().unwrap_or_else(|| ctx.undefined())
}
fn emit(
    ctx: &mut Context<'_>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<(), RootedError> {
    let f = get(ctx, object, "emit")?;
    let n = ctx.string_rooted(name);
    let mut all = vec![n];
    all.extend_from_slice(args);
    ctx.call_rooted(f, object, &all)?;
    Ok(())
}
fn transfers(ctx: &mut Context<'_>, value: RootId) -> Result<Vec<RootId>, RootedError> {
    if ctx.rooted_value(value).is_some_and(|v| v.is_undefined()) {
        return Ok(Vec::new());
    }
    let value = if ctx.is_array_rooted(value)? {
        value
    } else {
        get(ctx, value, "transfer")?
    };
    if ctx.rooted_value(value).is_some_and(|v| v.is_undefined()) {
        return Ok(Vec::new());
    }
    let iterator = ctx.iterator_rooted(value)?;
    let mut out = Vec::new();
    while let Some(item) = ctx.iterator_step_rooted(&iterator)? {
        out.push(item)
    }
    Ok(out)
}

// Constructor policy is ArrayPrototypeValues, not postMessage's transfer-options policy.
// Use the actual pinned SDK SafeArrayIterator so array-like length/index getters,
// ignored Symbol.iterator overrides and falsy lists have one existing authority.
fn constructor_transfers(ctx: &mut Context<'_>, value: RootId) -> Result<Vec<RootId>, RootedError> {
    if !ctx.truthy_rooted(value)? {
        return Ok(Vec::new());
    }
    let primordials = crate::host::shared_vm::commonjs::node_primordials(ctx)?
        .ok_or_else(|| RootedError::host("Worker requires pinned Node primordials"))?;
    let ctor = get(ctx, primordials, "SafeArrayIterator")?;
    let safe = ctx.construct_rooted(ctor, ctor, &[value])?;
    let iterator = ctx.iterator_rooted(safe)?;
    let mut out = Vec::new();
    while let Some(item) = ctx.iterator_step_rooted(&iterator)? {
        out.push(item)
    }
    Ok(out)
}
pub(crate) fn module(ctx: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = ctx.object_rooted()?;
    let emitter = super::events_shared_vm::module(ctx)?;
    let ctor = ctx.host_constructor_named(
        crate::host::shared_vm::operation("workerConstruct"),
        "Worker",
    )?;
    let proto = get(ctx, ctor, "prototype")?;
    let emitter_proto = get(ctx, emitter, "prototype")?;
    ctx.set_prototype_rooted(proto, emitter_proto)?;
    ctx.set_prototype_rooted(ctor, emitter)?;
    let prototype_key = ctx.string_rooted("prototype");
    ctx.define_data_property_rooted(ctor, prototype_key, proto, false, false, false)?;
    for (name, op) in [
        ("postMessage", "workerPost"),
        ("ref", "workerRef"),
        ("unref", "workerUnref"),
        ("cpuUsage", "workerCpuUsage"),
        ("terminate", "workerTerminate"),
    ] {
        method(ctx, proto, name, op)?
    }
    let global = ctx.global_root()?;
    // The BroadcastChannel polyfill delivers within a VM itself. This hook
    // carries messages across the worker boundary using the existing owned
    // clone channel.
    let broadcast_post = ctx.host_function_named(
        crate::host::shared_vm::operation("workerBroadcastPost"),
        "__quenchBroadcastPost",
    )?;
    define_hidden(ctx, global, "__quenchBroadcastPost", broadcast_post)?;
    let broadcast_ref = ctx.host_function_named(
        crate::host::shared_vm::operation("workerBroadcastRef"),
        "__quenchBroadcastRef",
    )?;
    define_hidden(ctx, global, "__quenchBroadcastRef", broadcast_ref)?;
    let symbol = get(ctx, global, "Symbol")?;
    let async_dispose = get(ctx, symbol, "asyncDispose")?;
    let terminate = get(ctx, proto, "terminate")?;
    if !ctx.define_data_property_rooted(proto, async_dispose, terminate, true, false, true)? {
        return Err(RootedError::host(
            "Worker async dispose definition rejected",
        ));
    }
    let getter = ctx.host_function_named(
        crate::host::shared_vm::operation("workerThreadId"),
        "get threadId",
    )?;
    let descriptor = ctx.object_rooted()?;
    define(ctx, descriptor, "get", getter)?;
    let yes = ctx.boolean(true);
    define(ctx, descriptor, "configurable", yes)?;
    let key = ctx.string_rooted("threadId");
    ctx.define_property_descriptor_rooted(proto, key, descriptor)?;
    let getter = ctx.host_function_named(
        crate::host::shared_vm::operation("workerThreadName"),
        "get threadName",
    )?;
    let descriptor = ctx.object_rooted()?;
    define(ctx, descriptor, "get", getter)?;
    let yes = ctx.boolean(true);
    define(ctx, descriptor, "configurable", yes)?;
    let key = ctx.string_rooted("threadName");
    ctx.define_property_descriptor_rooted(proto, key, descriptor)?;
    define(ctx, module, "Worker", ctor)?;
    let global = ctx.global_root()?;
    let symbol = get(ctx, global, "Symbol")?;
    let symbol_for = get(ctx, symbol, "for")?;
    let key = ctx.string_rooted("nodejs.worker_threads.SHARE_ENV");
    let share_env = ctx.call_rooted(symbol_for, symbol, &[key])?;
    define(ctx, module, "SHARE_ENV", share_env)?;
    let set_environment_data = ctx.host_function(crate::host::shared_vm::operation(
        "workerSetEnvironmentData",
    ))?;
    define(ctx, module, "setEnvironmentData", set_environment_data)?;
    let get_environment_data = ctx.host_function(crate::host::shared_vm::operation(
        "workerGetEnvironmentData",
    ))?;
    define(ctx, module, "getEnvironmentData", get_environment_data)?;
    let shared = ctx.host_mut().shared_state();
    let thread_id = shared.borrow().workers.thread_id;
    if thread_id != 0 {
        // Node wraps process.cwd in worker threads so an AtomicsLoad observes
        // cwd changes made by the parent. ProcessCwd is shared across Quench
        // worker runtimes, so each call can refresh through the original host
        // method while retaining Node's observable worker wrapper shape.
        let global = ctx.global_root()?;
        let process = get(ctx, global, "process")?;
        let cwd = get(ctx, process, "cwd")?;
        let make_worker_cwd = ctx.evaluate_script_rooted(
            "(originalCwd) => { const cwdCounter = new Int32Array(new SharedArrayBuffer(4)); const AtomicsLoad = Atomics.load; return function wrappedWorkerCwd() { AtomicsLoad(cwdCounter, 0); return originalCwd(); }; }",
            "node:worker_threads/process-cwd.js",
        )?;
        let undefined = ctx.undefined();
        let worker_cwd = ctx.call_rooted(make_worker_cwd, undefined, &[cwd])?;
        let cwd_key = ctx.string_rooted("cwd");
        if !ctx.set_property_rooted(process, cwd_key, worker_cwd, process)? {
            return Err(RootedError::host("cannot install worker process.cwd wrapper"));
        }
    }
    let main = ctx.boolean(thread_id == 0);
    define(ctx, module, "isMainThread", main)?;
    let id = ctx.number(thread_id as f64);
    define(ctx, module, "threadId", id)?;
    let thread_name = shared.borrow().workers.thread_name.clone();
    let thread_name = match thread_name {
        Some(name) => ctx.string_rooted(&name),
        None => ctx.null(),
    };
    define(ctx, module, "threadName", thread_name)?;
    let data = shared
        .borrow_mut()
        .workers
        .child
        .as_mut()
        .and_then(|c| c.data.take());
    let data = match data {
        Some(v) => clone::decode(ctx, &v)?,
        None => ctx.null(),
    };
    define(ctx, module, "workerData", data)?;
    if thread_id != 0 {
        let environment_data = shared
            .borrow()
            .workers
            .child
            .as_ref()
            .map(|child| child.environment_data.clone())
            .unwrap_or_default();
        shared.borrow_mut().workers.environment_data = environment_data;
    }
    let port = if thread_id == 0 {
        ctx.null()
    } else {
        let global = ctx.global_root()?;
        // Node creates parentPort in native code, while the public JS
        // constructor rejects user-created ports. The private factory runs
        // the normal constructor so EventTarget and MessagePort state exists.
        let create_port = get(ctx, global, "__quenchCreateMessagePort")?;
        let undefined = ctx.undefined();
        let port = ctx.call_rooted(create_port, undefined, &[])?;
        for (name, op) in [
            ("postMessage", "workerPortPost"),
            ("on", "workerPortOn"),
            ("ref", "workerPortRef"),
            ("unref", "workerPortUnref"),
            ("close", "workerPortClose"),
            ("hasRef", "workerPortHasRef"),
        ] {
            method(ctx, port, name, op)?
        }
        // Node's internal worker binding has a second message path used by
        // worker bootstrap/control messages. Keep it separate from public
        // parentPort.postMessage(), whose payloads are user messages.
        let post = ctx.host_function_named(
            crate::host::shared_vm::operation("workerInternalPost"),
            "__quenchInternalPost",
        )?;
        let key = ctx.string_rooted("__quenchInternalPost");
        ctx.define_data_property_rooted(port, key, post, true, false, true)?;
        let root = ctx.retain(port)?;
        shared.borrow_mut().workers.port = Some(root);
        port
    };
    define(ctx, module, "parentPort", port)?;
    for name in [
        "markAsUncloneable",
        "markAsUntransferable",
        "isMarkedAsUntransferable",
    ] {
        // The SDK marker functions are ordinary constructible functions.
        let op = ctx.host_constructor(crate::host::shared_vm::operation(name))?;
        define(ctx, module, name, op)?;
    }
    let global = ctx.global_root()?;
    for name in ["BroadcastChannel", "MessageChannel", "MessagePort"] {
        let v = if name == "BroadcastChannel" {
            get(ctx, global, "__quenchBroadcastChannelClass")?
        } else {
            get(ctx, global, name)?
        };
        define(ctx, module, name, v)?
    }
    let receive_message = get(ctx, global, "__quenchReceiveMessageOnPort")?;
    define(ctx, module, "receiveMessageOnPort", receive_message)?;
    method(
        ctx,
        module,
        "moveMessagePortToContext",
        "workerMovePortToContext",
    )?;
    Ok(module)
}

pub(crate) fn move_port_to_context(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let port = args.first().copied().unwrap_or_else(|| ctx.undefined());
    let closed = get(ctx, port, "_closed")?;
    if ctx.truthy_rooted(closed)? {
        let error = ctx.error_rooted("Cannot send data on closed MessagePort")?;
        let code = ctx.string_rooted("ERR_CLOSED_MESSAGE_PORT");
        let key = ctx.string_rooted("code");
        ctx.define_data_property_rooted(error, key, code, true, true, true)?;
        return Err(ctx.throw(error));
    }
    let error =
        ctx.type_error_rooted("moveMessagePortToContext requires native context support")?;
    let code = ctx.string_rooted("ERR_NOT_IMPLEMENTED");
    let key = ctx.string_rooted("code");
    ctx.define_data_property_rooted(error, key, code, true, true, true)?;
    Err(ctx.throw(error))
}
// Node lib/internal/worker.js filename policy. The URL predicate is the shared
// Rust translation of internal/url.js:isURL; path/URL operations and coded
// constructors retain their existing native and original-SDK authorities.
#[derive(Clone, Copy)]
enum FilenameMode {
    Classic,
    File,
    DataUrl,
}

fn decode_data_url(input: &str) -> Result<(String, bool), String> {
    use base64::Engine;

    let Some((metadata, payload)) = input.strip_prefix("data:").and_then(|v| v.split_once(','))
    else {
        return Err("Invalid data URL".to_owned());
    };
    let mut metadata_parts = metadata.split(';');
    let media_type = metadata_parts.next().unwrap_or_default();
    let base64_encoded = metadata_parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let javascript = matches!(
        media_type.to_ascii_lowercase().as_str(),
        "text/javascript" | "application/javascript" | "text/ecmascript" | "application/ecmascript"
    );

    let bytes = if base64_encoded {
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|_| "Invalid base64 data URL payload".to_owned())?
    } else {
        let mut bytes = Vec::with_capacity(payload.len());
        let source = payload.as_bytes();
        let mut index = 0;
        while index < source.len() {
            if source[index] == b'%' {
                if index + 2 >= source.len() {
                    return Err("Invalid percent escape in data URL".to_owned());
                }
                let hex = std::str::from_utf8(&source[index + 1..index + 3])
                    .map_err(|_| "Invalid percent escape in data URL".to_owned())?;
                bytes.push(
                    u8::from_str_radix(hex, 16)
                        .map_err(|_| "Invalid percent escape in data URL".to_owned())?,
                );
                index += 3;
            } else {
                bytes.push(source[index]);
                index += 1;
            }
        }
        bytes
    };
    let source =
        String::from_utf8(bytes).map_err(|_| "Data URL payload is not valid UTF-8".to_owned())?;
    Ok((source, javascript))
}
fn filename_capabilities(ctx: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        ctx,
        "\0worker/filenameCapabilities",
        |ctx| {
            ctx.with_startup_intrinsics(|ctx| {
                let capabilities = ctx.null_object_rooted()?;
                let path = super::path_shared_vm::module(ctx)?;
                define(ctx, capabilities, "path", path)?;
                let codes = crate::host::shared_vm::commonjs::node_library_export(
                    ctx,
                    "internal/worker.js",
                    "internal/errors.js",
                    "codes",
                )?;
                for name in [
                    "ERR_INVALID_ARG_VALUE",
                    "ERR_INVALID_ARG_TYPE",
                    "ERR_WORKER_PATH",
                ] {
                    let constructor = get(ctx, codes, name)?;
                    define(ctx, capabilities, name, constructor)?;
                }
                let validate_array = crate::host::shared_vm::commonjs::node_library_export(
                    ctx,
                    "internal/worker.js",
                    "internal/validators.js",
                    "validateArray",
                )?;
                define(ctx, capabilities, "validateArray", validate_array)?;
                for name in ["fileURLToPath", "pathToFileURL"] {
                    let function = ctx.host_function(crate::host::shared_vm::operation(name))?;
                    define(ctx, capabilities, name, function)?;
                }
                Ok(capabilities)
            })
        },
    )
}
fn filename_error(
    ctx: &mut Context<'_>,
    capabilities: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootedError, RootedError> {
    let constructor = get(ctx, capabilities, name)?;
    let error = ctx.construct_rooted(constructor, constructor, args)?;
    Ok(ctx.throw(error))
}
fn call_path_method(
    ctx: &mut Context<'_>,
    path: RootId,
    name: &str,
    filename: RootId,
) -> Result<RootId, RootedError> {
    let function = get(ctx, path, name)?;
    if !ctx.is_callable_rooted(function)? {
        let error = ctx.type_error_rooted(&format!("path.{name} is not a function"))?;
        return Err(ctx.throw(error));
    }
    ctx.call_rooted(function, path, &[filename])
}
fn normalize_filename(
    ctx: &mut Context<'_>,
    filename: RootId,
    options: RootId,
) -> Result<(RootId, FilenameMode), RootedError> {
    let capabilities = filename_capabilities(ctx)?;
    let eval = get(ctx, options, "eval")?;
    if ctx.truthy_rooted(eval)? {
        if ctx.string_text(filename)?.is_none() {
            let label = ctx.string_rooted("options.eval");
            // The SDK reads options.eval a second time when constructing this error.
            let value = get(ctx, options, "eval")?;
            let reason = ctx.string_rooted("must be false when 'filename' is not a string");
            return Err(filename_error(
                ctx,
                capabilities,
                "ERR_INVALID_ARG_VALUE",
                &[label, value, reason],
            )?);
        }
        return Ok((filename, FilenameMode::Classic));
    }
    if super::url_shared_vm::is_url_like(ctx, filename)? {
        let protocol = get(ctx, filename, "protocol")?;
        if ctx.string_text(protocol)?.as_deref() == Some("data:") {
            let filename = ctx.to_string(filename)?;
            return Ok((ctx.string_rooted(&filename), FilenameMode::DataUrl));
        }
    }
    let undefined = ctx.undefined();
    if super::url_shared_vm::is_url_like(ctx, filename)? {
        let convert = get(ctx, capabilities, "fileURLToPath")?;
        let filename = ctx.call_rooted(convert, undefined, &[filename])?;
        return Ok((filename, FilenameMode::File));
    }
    let Some(text) = ctx.string_text(filename)? else {
        let label = ctx.string_rooted("filename");
        let string = ctx.string_rooted("string");
        let url = ctx.string_rooted("URL");
        let types = ctx.array_rooted(&[string, url])?;
        return Err(filename_error(
            ctx,
            capabilities,
            "ERR_INVALID_ARG_TYPE",
            &[label, types, filename],
        )?);
    };
    let path = get(ctx, capabilities, "path")?;
    let absolute = call_path_method(ctx, path, "isAbsolute", filename)?;
    // Exact admission of the SDK's /^\.\.?[\\/]/ intrinsic regexp.
    let relative = text.starts_with("./")
        || text.starts_with(".\\")
        || text.starts_with("../")
        || text.starts_with("..\\");
    if ctx.truthy_rooted(absolute)? || relative {
        let filename = call_path_method(ctx, path, "resolve", filename)?;
        let convert = get(ctx, capabilities, "pathToFileURL")?;
        ctx.call_rooted(convert, undefined, &[filename])?;
        Ok((filename, FilenameMode::File))
    } else {
        Err(filename_error(
            ctx,
            capabilities,
            "ERR_WORKER_PATH",
            &[filename],
        )?)
    }
}

fn constructor_argv(ctx: &mut Context<'_>, options: RootId) -> Result<Vec<String>, RootedError> {
    let value = get(ctx, options, "argv")?;
    if !ctx.truthy_rooted(value)? {
        return Ok(Vec::new());
    }
    let capabilities = filename_capabilities(ctx)?;
    let validate = get(ctx, capabilities, "validateArray")?;
    let label = ctx.string_rooted("options.argv");
    let undefined = ctx.undefined();
    ctx.call_rooted(validate, undefined, &[value, label])?;
    let length = get(ctx, value, "length")?;
    let length = ctx
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0)
        .map(|length| length.trunc() as usize)
        .unwrap_or(0);
    let string = ctx.with_startup_intrinsics(|ctx| {
        let global = ctx.global_root()?;
        let key = ctx.string_rooted("String");
        ctx.get_property_rooted(global, key)
    })?;
    let mut argv = Vec::with_capacity(length);
    for index in 0..length {
        let key = ctx.string_rooted(&index.to_string());
        let argument = ctx.get_property_rooted(value, key)?;
        let argument = ctx.call_rooted(string, undefined, &[argument])?;
        argv.push(
            ctx.string_text(argument)?
                .expect("String constructor returns a string"),
        );
    }
    Ok(argv)
}

fn constructor_exec_argv(
    ctx: &mut Context<'_>,
    options: RootId,
) -> Result<Option<Vec<String>>, RootedError> {
    let mut value = get(ctx, options, "execArgv")?;
    let explicit = !ctx
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined());
    if !explicit {
        // Node inherits process.execArgv when execArgv is omitted. Inherited
        // flags are already accepted by the parent process, so preserve them
        // without applying the stricter validation used for explicit options.
        let global = ctx.global_root()?;
        let process = get(ctx, global, "process")?;
        value = get(ctx, process, "execArgv")?;
        if ctx
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            return Ok(None);
        }
    }
    let capabilities = filename_capabilities(ctx)?;
    let validate = get(ctx, capabilities, "validateArray")?;
    let label = ctx.string_rooted("options.execArgv");
    let undefined = ctx.undefined();
    ctx.call_rooted(validate, undefined, &[value, label])?;
    let length = get(ctx, value, "length")?;
    let length = ctx
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0)
        .map(|length| length.trunc() as usize)
        .unwrap_or(0);
    let string = ctx.with_startup_intrinsics(|ctx| {
        let global = ctx.global_root()?;
        let key = ctx.string_rooted("String");
        ctx.get_property_rooted(global, key)
    })?;
    let mut argv = Vec::with_capacity(length);
    for index in 0..length {
        let key = ctx.string_rooted(&index.to_string());
        let argument = ctx.get_property_rooted(value, key)?;
        let argument = ctx.call_rooted(string, undefined, &[argument])?;
        let argument = ctx
            .string_text(argument)?
            .expect("String constructor returns a string");
        let flag = argument
            .split_once('=')
            .map_or(argument.as_str(), |(flag, _)| flag);
        if explicit {
            let global = ctx.global_root()?;
            let process = get(ctx, global, "process")?;
            let allowed = get(ctx, process, "allowedNodeEnvironmentFlags")?;
            let has = get(ctx, allowed, "has")?;
            let flag_value = ctx.string_rooted(flag);
            let flag_allowed = if ctx.is_callable_rooted(has)? {
                let allowed = ctx.call_rooted(has, allowed, &[flag_value])?;
                ctx.truthy_rooted(allowed)?
            } else {
                matches!(
                    flag,
                    "--expose-internals"
                        | "--input-type"
                        | "--trace-warnings"
                        | "--experimental-detect-module"
                        | "--no-experimental-detect-module"
                )
            };
            let valid = (flag_allowed || flag == "--expose-internals")
                && !matches!(flag, "--title" | "--redirect-warnings" | "--expose-gc");
            if !valid {
                let error = ctx.error_rooted(&format!(
                    "Initiated Worker with invalid execArgv flags: {argument}"
                ))?;
                super::node_errors_shared_vm::decorate_error(
                    ctx,
                    error,
                    "ERR_WORKER_INVALID_EXEC_ARGV",
                )?;
                return Err(ctx.throw(error));
            }
        }
        argv.push(argument);
    }
    Ok(Some(argv))
}

fn constructor_env(
    ctx: &mut Context<'_>,
    options: RootId,
) -> Result<(Option<HashMap<String, String>>, bool), RootedError> {
    let mut value = get(ctx, options, "env")?;
    if ctx
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined() || value.is_null())
    {
        let global = ctx.global_root()?;
        let process = get(ctx, global, "process")?;
        value = get(ctx, process, "env")?;
    }
    let global = ctx.global_root()?;
    let symbol = get(ctx, global, "Symbol")?;
    let symbol_for = get(ctx, symbol, "for")?;
    let key = ctx.string_rooted("nodejs.worker_threads.SHARE_ENV");
    let share_env = ctx.call_rooted(symbol_for, symbol, &[key])?;
    if ctx.strict_equal_rooted(value, share_env)? {
        return Ok((None, true));
    }
    if !ctx.is_object_rooted(value)? {
        let received = if ctx
            .rooted_value(value)
            .and_then(|v| v.as_number())
            .is_some()
        {
            format!("type number ({})", ctx.to_string(value)?)
        } else if ctx.rooted_value(value).and_then(|v| v.as_bool()).is_some() {
            format!("type boolean ({})", ctx.to_string(value)?)
        } else {
            "type value".to_owned()
        };
        let error = ctx.type_error_rooted(&format!(
            "The \"options.env\" property must be of type object or one of undefined, null, or worker_threads.SHARE_ENV. Received {received}"
        ))?;
        super::node_errors_shared_vm::decorate_type_error(ctx, error, "ERR_INVALID_ARG_TYPE")?;
        return Err(ctx.throw(error));
    }
    let mut env = HashMap::new();
    for key in ctx.enumerable_own_keys_rooted(value, true, true)? {
        let Some(name) = ctx.string_text(key)? else {
            continue;
        };
        let entry = ctx.get_property_rooted(value, key)?;
        let entry = ctx.to_string(entry)?;
        env.insert(name, entry);
    }
    Ok((Some(env), false))
}

pub(crate) fn construct(
    ctx: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if ctx.new_target_rooted().is_none() {
        let error =
            ctx.type_error_rooted("Class constructor Worker cannot be invoked without new")?;
        return Err(ctx.throw(error));
    }
    let global = ctx.global_root()?;
    let process = get(ctx, global, "process")?;
    let permission = get(ctx, process, "permission")?;
    if ctx.is_object_rooted(permission)? {
        let has = get(ctx, permission, "has")?;
        let scope = ctx.string_rooted("worker");
        let allowed = ctx.call_rooted(has, permission, &[scope])?;
        let exec_argv = get(ctx, process, "execArgv")?;
        let audit_flag = ctx.string_rooted("--permission-audit");
        let audit = if ctx.is_object_rooted(exec_argv)? {
            let includes = get(ctx, exec_argv, "includes")?;
            let audit = ctx.call_rooted(includes, exec_argv, &[audit_flag])?;
            ctx.truthy_rooted(audit)?
        } else {
            false
        };
        if !ctx.truthy_rooted(allowed)? && !audit {
            let error = ctx.error_rooted(
                "Access to this API has been restricted. Use --allow-worker to manage permissions.",
            )?;
            super::node_errors_shared_vm::decorate_error(ctx, error, "ERR_ACCESS_DENIED")?;
            let key = ctx.string_rooted("permission");
            let value = ctx.string_rooted("WorkerThreads");
            ctx.define_data_property_rooted(error, key, value, true, true, true)?;
            return Err(ctx.throw(error));
        }
    }
    let source = arg(ctx, args, 0);
    let options = arg(ctx, args, 1);
    let options = if ctx.rooted_value(options).is_some_and(|v| v.is_undefined()) {
        ctx.object_rooted()?
    } else {
        options
    };
    let (source, mode) = normalize_filename(ctx, source, options)?;
    let mut source = ctx.to_string(source)?;
    let data_url = matches!(mode, FilenameMode::DataUrl);
    let eval = matches!(mode, FilenameMode::Classic | FilenameMode::DataUrl);
    let data_url_has_javascript_mime = if data_url {
        match decode_data_url(&source) {
            Ok((decoded, javascript_mime)) => {
                source = decoded;
                javascript_mime
            }
            Err(message) => return Err(fail(ctx, &message)?),
        }
    } else {
        true
    };
    if data_url && !data_url_has_javascript_mime {
        // Node treats non-JavaScript media types as module-load failures
        // delivered through the Worker error event rather than rejecting
        // the constructor synchronously.
        source = "throw new TypeError('Unknown module format for data URL')".to_owned();
    }
    let entry_path = (!eval).then(|| std::path::PathBuf::from(&source));
    let list = get(ctx, options, "transferList")?;
    let transfers = constructor_transfers(ctx, list)?;
    let data = get(ctx, options, "workerData")?;
    let worker_argv = constructor_argv(ctx, options)?;
    let mut worker_exec_argv = constructor_exec_argv(ctx, options)?;
    let (worker_env, worker_env_shared) = constructor_env(ctx, options)?;
    if worker_env_shared {
        let global = ctx.global_root()?;
        let process = get(ctx, global, "process")?;
        let key = ctx.string_rooted("__quenchEnvShared");
        let yes = ctx.boolean(true);
        if !ctx.set_property_rooted(process, key, yes, process)? {
            return Err(RootedError::host("cannot enable shared process.env"));
        }
        // The JS proxy consults both this property and the native state flag.
        // Keep the parent and child views pointed at the process-wide
        // environment when SHARE_ENV is requested.
        ctx.host_mut()
            .shared_state()
            .borrow_mut()
            .process_env_shared = true;
    }
    if let Some(node_options) = worker_env.as_ref().and_then(|env| env.get("NODE_OPTIONS")) {
        let global = ctx.global_root()?;
        let process = get(ctx, global, "process")?;
        let allowed = get(ctx, process, "allowedNodeEnvironmentFlags")?;
        let has = get(ctx, allowed, "has")?;
        for argument in node_options_tokens(node_options) {
            let flag = argument
                .split_once('=')
                .map_or(argument.as_str(), |(flag, _)| flag);
            let flag_value = ctx.string_rooted(flag);
            let valid = if ctx.is_callable_rooted(has)? {
                let allowed = ctx.call_rooted(has, allowed, &[flag_value])?;
                ctx.truthy_rooted(allowed)?
            } else {
                matches!(flag, "--trace-exit" | "--trace-warnings" | "--no-warnings")
            };
            if !valid {
                let error = ctx.error_rooted(&format!(
                    "Initiated Worker with invalid NODE_OPTIONS env variable: {argument} is not allowed in NODE_OPTIONS"
                ))?;
                super::node_errors_shared_vm::decorate_error(
                    ctx,
                    error,
                    "ERR_WORKER_INVALID_EXEC_ARGV",
                )?;
                return Err(ctx.throw(error));
            }
        }
    }
    // Node applies isolate-safe NODE_OPTIONS from a Worker's environment to
    // that worker. Preserve --trace-exit here so process.exit in the worker
    // follows the same option as a regular Node isolate.
    if worker_env
        .as_ref()
        .and_then(|env| env.get("NODE_OPTIONS"))
        .is_some_and(|value| node_options_has_flag(value, "--trace-exit"))
    {
        let flags = worker_exec_argv.get_or_insert_with(Vec::new);
        if !flags.iter().any(|flag| flag == "--trace-exit") {
            flags.push("--trace-exit".to_owned());
        }
    }
    // `NODE_OPTIONS` is inherited by the worker isolate independently of
    // `execArgv`. Preserve the child process environment's supported options
    // while leaving explicit Worker execArgv visible as-is. The V8 option
    // below is advertised by Node but has no matching host-side option.
    let child_node_options = worker_env
        .as_ref()
        .and_then(|env| env.get("NODE_OPTIONS"))
        .map(|value| {
            node_options_tokens(value)
                .into_iter()
                .filter(|argument| argument != "--no-extra-info-on-fatal-exception")
                .collect::<Vec<_>>()
                .join(" ")
        });
    let worker_eval_kind = if data_url {
        quench_runtime::SourceKind::Module
    } else {
        eval_source_kind(&source, worker_exec_argv.as_deref())
    };
    let name = get(ctx, options, "name")?;
    let worker_name = if ctx
        .rooted_value(name)
        .is_some_and(|value| value.is_undefined())
    {
        None
    } else {
        ctx.string_text(name)?
    };
    // Reject explicitly rather than silently ignore environment/stdio/resource policy.
    // Node exposes a Readable on Worker.stdout regardless of the stdout option;
    // when stdout is false it also pipes that stream to process.stdout.
    let stdout_option = get(ctx, options, "stdout")?;
    let stdout_enabled = ctx.truthy_rooted(stdout_option)?;
    for name in ["stdin", "resourceLimits", "trackUnmanagedFds"] {
        let v = get(ctx, options, name)?;
        if !ctx.rooted_value(v).is_some_and(|v| v.is_undefined()) {
            return Err(fail(
                ctx,
                "Worker options outside eval/workerData are not implemented",
            )?);
        }
    }
    let data = capture_message(ctx, data, &transfers)?;
    let emitter = super::events_shared_vm::module(ctx)?;
    ctx.call_rooted(emitter, receiver, &[])?;
    let stderr = ctx.construct_rooted(emitter, emitter, &[])?;
    define(ctx, receiver, "stderr", stderr)?;
    let readable = crate::host::shared_vm::commonjs::node_library_export(
        ctx,
        "worker_threads.js",
        "stream.js",
        "Readable",
    )?;
    let options_factory = ctx.evaluate_script_rooted(
        "() => ({ read() {} })",
        "node:worker_threads/stdio-options.js",
    )?;
    let undefined = ctx.undefined();
    let options = ctx.call_rooted(options_factory, undefined, &[])?;
    let stdout = ctx.construct_rooted(readable, readable, &[options])?;
    if !stdout_enabled {
        let global = ctx.global_root()?;
        let process = get(ctx, global, "process")?;
        let process_stdout = get(ctx, process, "stdout")?;
        let pipe = get(ctx, stdout, "pipe")?;
        ctx.call_rooted(pipe, stdout, &[process_stdout])?;
    }
    define(ctx, receiver, "stdout", stdout)?;
    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
    let inherited_expose_gc = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .exec_argv
        .iter()
        .any(|flag| flag == "--expose-gc" || flag == "--expose_gc");
    let interrupt = quench_runtime::ExecutionInterrupt::default();
    let cpu_tid = std::sync::Arc::new(AtomicU64::new(0));
    let child_cpu_tid = cpu_tid.clone();
    let child_interrupt = interrupt.clone();
    let child_thread_name = worker_name.clone();
    let child_exec_argv = worker_exec_argv.clone();
    let child_eval_kind = worker_eval_kind;
    let child_inherited_expose_gc = inherited_expose_gc;
    let child_env = worker_env.clone();
    let child_node_options = child_node_options.clone();
    let child_environment_data = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .workers
        .environment_data
        .clone();
    // Workers inherit the parent's current directory at construction time.
    // The runtime models cwd per NodeHost, so capture it before crossing the
    // thread boundary instead of letting the child initialize from the
    // process-wide launch directory.
    let child_cwd = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .cwd
        .clone();
    let (tx, rx) = mpsc::channel();
    let (up_tx, up_rx) = mpsc::channel();
    let library = ctx.host_mut().node_library_root.clone();
    let child_library = library;
    let child_performance_origins = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .performance_origins();
    let worker_output = up_tx.clone();
    let worker_stdout = up_tx.clone();
    let global = ctx.global_root()?;
    let process = get(ctx, global, "process")?;
    let exec_path = get(ctx, process, "execPath")?;
    let exec_path = ctx
        .string_text(exec_path)?
        .ok_or_else(|| RootedError::host("process.execPath is unavailable"))?;
    let mut argv = vec![exec_path];
    argv.push(if eval {
        "[worker eval]".to_owned()
    } else {
        source.clone()
    });
    argv.extend(worker_argv);
    let join = thread::Builder::new()
        .name(
            worker_name
                .clone()
                .unwrap_or_else(|| format!("quench-worker-{thread_id}")),
        )
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || {
            #[cfg(target_os = "linux")]
            child_cpu_tid.store(
                unsafe { libc::syscall(libc::SYS_gettid) as u64 },
                Ordering::Release,
            );
            let report = up_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut host = NodeHost::new(argv)
                    .with_initial_node_options(child_node_options)
                    .with_output_sink(std::sync::Arc::new(move |text| {
                        worker_output.send(Event::Stderr(text.to_owned())).ok();
                    }))
                    .with_stdout_sink(std::sync::Arc::new(move |text| {
                        worker_stdout.send(Event::Stdout(text.to_owned())).ok();
                    }));
                host.shared_state.borrow_mut().cwd = child_cwd;
                host.shared_state.borrow_mut().process_control =
                    super::process_state::ProcessControl::with_performance_origins(
                        child_performance_origins,
                    );
                host.node_library_root = child_library;
                host = host
                    .with_exec_argv(child_exec_argv.clone().unwrap_or_default())
                    .map_err(Failure::host)?;
                let (program_source, program_name, kind, eval_script) = if let Some(path) =
                    entry_path
                {
                    if let Err(error) = std::fs::metadata(&path) {
                        let message = if error.kind() == std::io::ErrorKind::NotFound {
                            format!("Cannot find module '{}'", path.display())
                        } else {
                            error.to_string()
                        };
                        return Err(Failure::host(message));
                    }
                    let kind = NodeHost::source_kind(&path).map_err(Failure::host)?;
                    let name = path.to_string_lossy().into_owned();
                    let text = if kind == quench_runtime::SourceKind::Module {
                        std::fs::read_to_string(&path).map_err(|e| Failure::host(e.to_string()))?
                    } else {
                        host = host.with_commonjs_entry(path);
                        String::new()
                    };
                    (text, name, kind, None)
                } else {
                    let name = "[worker eval]".to_owned();
                    if child_eval_kind == quench_runtime::SourceKind::Module {
                        (source, name, child_eval_kind, None)
                    } else {
                        (
                            String::new(),
                            name,
                            quench_runtime::SourceKind::Script,
                            Some(source),
                        )
                    }
                };

                {
                    let mut state = host.shared_state.borrow_mut();
                    state.workers.thread_id = thread_id;
                    state.workers.thread_name = child_thread_name;
                    state.workers.exec_argv = child_exec_argv;
                    state.workers.inherited_expose_gc = child_inherited_expose_gc;
                    state.process_env_override = child_env;
                    state.process_env_shared = worker_env_shared;
                    state.workers.eval_script = eval_script;
                    state.workers.child = Some(Child {
                        data: Some(data),
                        environment_data: child_environment_data,
                        rx,
                        tx: up_tx,
                        interrupt: child_interrupt.clone(),
                        pending_messages: VecDeque::new(),
                        broadcast_refs: 0,
                        listening: false,
                        refed: false,
                        closed: false,
                    });
                }
                let mut runtime =
                    Runtime::new_with_execution_interrupt(host, child_interrupt.clone());
                let interrupt_handle = child_interrupt;
                let program = Engine::compile(ExecutionRequest {
                    source: &program_source,
                    name: &program_name,
                    kind,
                })
                .map_err(|e| Failure::host(format!("{e:?}")))?;
                report.send(Event::Online).ok();
                let mut callback_exception = None;
                let execution = runtime.execute_deferred_jobs(&program).and_then(|()| {
                    if runtime
                        .host_mut()
                        .shared_state()
                        .borrow()
                        .workers
                        .eval_script
                        .is_none()
                    {
                        return Ok(());
                    }
                    let function = runtime
                        .host_function(crate::host::shared_vm::operation("workerEvaluate"))
                        .expect("registered Worker evaluator");
                    let undefined = runtime.root(quench_runtime::Value::UNDEFINED);
                    let result = runtime.call_rooted(function, undefined, &[]);
                    runtime.release_root(function);
                    runtime.release_root(undefined);
                    match result {
                        Ok(value) => {
                            runtime.release_root(value);
                            Ok(())
                        }
                        Err(error) => {
                            callback_exception = error.exception;
                            Err(error.error)
                        }
                    }
                });
                let completion = match execution {
                    Ok(()) => super::process_shared_vm::finish_execution(&mut runtime, &program),
                    Err(error) => {
                        let exception = error.thrown_value().map(|v| runtime.root(v));
                        let callback_exception = callback_exception.take();
                        if error.is_execution_terminated() {
                            if let Some(root) = exception {
                                runtime.release_root(root);
                            }
                            if let Some(root) = callback_exception {
                                runtime.release_root(root);
                            }
                            let control = runtime
                                .host_mut()
                                .shared_state()
                                .borrow()
                                .process_control
                                .clone();
                            if let Some(code) = control.requested_exit_code() {
                                super::process_shared_vm::finish_requested_exit(
                                    &mut runtime,
                                    &program,
                                )
                                .map_err(Failure::host)?;
                                return Ok(control.exit_code().unwrap_or(code));
                            }
                            return Ok(1);
                        }
                        let shared = runtime.host_mut().shared_state();
                        let outcome = if shared
                            .borrow()
                            .process_control
                            .requested_exit_code()
                            .is_some()
                        {
                            if let Some(root) = callback_exception {
                                runtime.release_root(root);
                            }
                            super::process_shared_vm::finish_requested_exit(&mut runtime, &program)
                                .map_err(|message| completion_failure(&mut runtime, message))
                        } else {
                            let handled = super::process_shared_vm::finish_after_uncaught_error(
                                &mut runtime,
                                &program,
                                &error,
                            );
                            match handled {
                                Ok(true) => {
                                    if let Some(root) = callback_exception {
                                        runtime.release_root(root);
                                    }
                                    Ok(())
                                }
                                Ok(false) => Err(failure(&mut runtime, error, callback_exception)),
                                Err(message) => {
                                    if let Some(root) = callback_exception {
                                        runtime.release_root(root);
                                    }
                                    Err(completion_failure(&mut runtime, message))
                                }
                            }
                        };
                        if let Some(root) = exception {
                            runtime.release_root(root);
                        }
                        if interrupt_handle.is_termination_requested() {
                            return Ok(runtime
                                .host_mut()
                                .shared_state()
                                .borrow()
                                .process_control
                                .requested_exit_code()
                                .unwrap_or(1));
                        }
                        outcome?;
                        Ok(())
                    }
                };
                if interrupt_handle.is_termination_requested() {
                    return Ok(runtime
                        .host_mut()
                        .shared_state()
                        .borrow()
                        .process_control
                        .requested_exit_code()
                        .unwrap_or(1));
                }
                let control = runtime
                    .host_mut()
                    .shared_state()
                    .borrow()
                    .process_control
                    .clone();
                if control.fatal_uncaught_requested() || control.fatal_exception_handler_failed() {
                    super::process_shared_vm::finish_requested_exit(&mut runtime, &program)
                        .map_err(Failure::host)?;
                    return Ok(control.exit_code().unwrap_or(WORKER_FAILURE_EXIT_CODE));
                }
                completion.map_err(|message| completion_failure(&mut runtime, message))?;
                if child_eval_kind == quench_runtime::SourceKind::Module
                    && runtime
                        .module_evaluation_pending(&program)
                        .map_err(|error| Failure::host(error.to_string()))?
                {
                    // Node exits a worker with status 13 when its top-level
                    // module await remains unsettled after all queued jobs run.
                    return Ok(13);
                }
                Ok::<_, Failure>(runtime.host_mut().exit_code().unwrap_or(0))
            }));
            match result {
                Ok(Ok(code)) => {
                    report.send(Event::Exit(code)).ok();
                }
                Ok(Err(e)) => {
                    let code = e.exit_code;
                    report.send(Event::Error(e)).ok();
                    report.send(Event::Exit(code)).ok();
                }
                Err(_) => {
                    report
                        .send(Event::Error(Failure::host(
                            "Worker runtime panicked".into(),
                        )))
                        .ok();
                    report.send(Event::Exit(1)).ok();
                }
            }
        })
        .map_err(|e| RootedError::host(e.to_string()))?;
    let identity = ctx.host_weak_root(receiver)?;
    let root = ctx.retain(receiver)?;
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .workers
        .insert(
            thread_id,
            Worker {
                identity,
                root: Some(root),
                tx,
                rx: up_rx,
                interrupt,
                join: Some(join),
                alive: true,
                cpu_tid,
                handle_active: true,
                async_resource: None,
                message_port_async_resource: None,
                message_port_refed: false,
                thread_name: worker_name,
                refed: true,
                resolvers: Vec::new(),
            },
        );
    // Node exposes the WorkerImpl as the resource for a native WORKER async
    // hook. Keep that private resource separate from the public Worker object
    // and let its hasRef method reflect this worker's native ref state.
    let resource = ctx.object_rooted()?;
    let thread_id_value = ctx.number(thread_id as f64);
    let has_ref = ctx.host_function_with_data(
        crate::host::shared_vm::operation("workerHandleHasRef"),
        thread_id_value,
    )?;
    let key = ctx.string_rooted("hasRef");
    if !ctx.define_data_property_rooted(resource, key, has_ref, true, false, true)? {
        return Err(RootedError::host(
            "Worker handle hasRef definition rejected",
        ));
    }
    let async_id = super::async_hooks_shared_vm::create_context(&ctx.host_mut().shared_state());
    let async_id_value = ctx.number(async_id as f64);
    let key = ctx.string_rooted("\0quench:async_hooks:id");
    ctx.define_data_property_rooted(resource, key, async_id_value, true, false, true)?;
    super::async_hooks_shared_vm::emit_init(ctx, resource, "WORKER", async_id)?;
    let resource_root = ctx.retain(resource)?;
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .workers
        .get_mut(&thread_id)
        .unwrap()
        .async_resource = Some(resource_root);
    let port_resource = ctx.object_rooted()?;
    let thread_id_value = ctx.number(thread_id as f64);
    let has_ref = ctx.host_function_with_data(
        crate::host::shared_vm::operation("workerPortHandleHasRef"),
        thread_id_value,
    )?;
    let key = ctx.string_rooted("hasRef");
    if !ctx.define_data_property_rooted(port_resource, key, has_ref, true, false, true)? {
        return Err(RootedError::host(
            "MessagePort handle hasRef definition rejected",
        ));
    }
    let async_id = super::async_hooks_shared_vm::create_context(&ctx.host_mut().shared_state());
    let async_id_value = ctx.number(async_id as f64);
    let key = ctx.string_rooted("\0quench:async_hooks:id");
    ctx.define_data_property_rooted(port_resource, key, async_id_value, true, false, true)?;
    super::async_hooks_shared_vm::emit_init(ctx, port_resource, "MESSAGEPORT", async_id)?;
    let port_resource_root = ctx.retain(port_resource)?;
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .workers
        .get_mut(&thread_id)
        .unwrap()
        .message_port_async_resource = Some(port_resource_root);
    let diagnostic_message = ctx.object_rooted()?;
    let worker_key = ctx.string_rooted("worker");
    ctx.define_data_property_rooted(diagnostic_message, worker_key, receiver, true, true, true)?;
    super::diagnostics_channel_shared_vm::publish_named(ctx, "worker_threads", diagnostic_message)?;
    // Node's Worker constructor publishes the new worker on `process` on the
    // next tick, before the worker's asynchronous `online` event. Keep this
    // in JS scheduling rather than emitting synchronously so listeners observe
    // the same constructor boundary as they do in Node.
    let process = get(ctx, global, "process")?;
    let next_tick = get(ctx, process, "nextTick")?;
    let callback = ctx.evaluate_script_rooted(
        "((worker) => process.emit('worker', worker))",
        "node:worker_threads/process-worker-event.js",
    )?;
    ctx.call_rooted(next_tick, process, &[callback, receiver])?;
    Ok(receiver)
}
pub(crate) fn is_child(ctx: &mut Context<'_>) -> bool {
    ctx.host_mut().shared_state().borrow().workers.thread_id != 0
}
pub(crate) fn raw_umask(
    ctx: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = args
        .first()
        .is_none_or(|r| ctx.rooted_value(*r).is_some_and(|v| v.is_undefined()));
    super::process_shared_vm::umask(ctx, receiver, if undefined { &[] } else { args })
}
pub(crate) fn initialize_process_owner(ctx: &mut Context<'_>) -> Result<(), RootedError> {
    if !is_child(ctx) {
        return Ok(());
    }
    crate::host::shared_vm::commonjs::node_library_module(
        ctx,
        "internal/main/worker_thread.js",
        "internal/bootstrap/switches/does_not_own_process_state.js",
    )?;
    let process = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("Worker process owner is not initialized"))?;
    let env = get(ctx, process, "env")?;
    let channel = get(ctx, env, "NODE_CHANNEL_FD")?;
    if ctx.truthy_rooted(channel)? {
        let unavailable = crate::host::shared_vm::commonjs::node_library_export(
            ctx,
            "internal/main/worker_thread.js",
            "internal/process/worker_thread_only.js",
            "unavailable",
        )?;
        let undefined = ctx.undefined();
        for name in ["send", "disconnect", "channel", "connected"] {
            let label = ctx.string_rooted(&format!(
                "process.{name}{}",
                if matches!(name, "send" | "disconnect") {
                    "()"
                } else {
                    ""
                }
            ));
            let function = ctx.call_rooted(unavailable, undefined, &[label])?;
            if matches!(name, "channel" | "connected") {
                let descriptor = ctx.object_rooted()?;
                define(ctx, descriptor, "get", function)?;
                let key = ctx.string_rooted(name);
                ctx.define_property_descriptor_rooted(process, key, descriptor)?;
            } else {
                define(ctx, process, name, function)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn serialize_failure(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let error = arg(ctx, args, 0);
    let packet = error_packet::capture(ctx, error)?;
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .serialized_failure = Some(packet);
    Ok(ctx.undefined())
}

pub(crate) fn evaluate(
    ctx: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let source = ctx
        .host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .eval_script
        .take()
        .ok_or_else(|| RootedError::host("Worker eval source already consumed"))?;
    // This is the real child's Core Script compiler/evaluator, which creates
    // typed syntax/stack errors through the canonical compilation boundary.
    // Worker eval is a top-level Script, not a dynamic `eval()` call. Using
    // the embedding Script compiler keeps error frames attributed to the
    // worker's `[worker eval]` source name instead of synthesizing an
    // `eval at <anonymous>` origin.
    ctx.evaluate_specialized_script_rooted(&source, "[worker eval]")
}
pub(crate) fn thread_id(
    ctx: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(ctx, receiver)?;
    let shared = ctx.host_mut().shared_state();
    let value = if shared.borrow().workers.workers[&id].alive {
        id as f64
    } else {
        -1.
    };
    Ok(ctx.number(value))
}
pub(crate) fn thread_name(
    ctx: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(ctx, receiver)?;
    let shared = ctx.host_mut().shared_state();
    let name = shared.borrow().workers.workers[&id].thread_name.clone();
    match name {
        Some(name) => Ok(ctx.string_rooted(&name)),
        None => Ok(ctx.null()),
    }
}
pub(crate) fn post(
    ctx: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(ctx, receiver)?;
    let shared = ctx.host_mut().shared_state();
    if !shared.borrow().workers.workers[&id].alive {
        return Ok(ctx.undefined());
    }
    let value = arg(ctx, args, 0);
    let list = arg(ctx, args, 1);
    let transfers = transfers(ctx, list)?;
    if shared.borrow().workers.workers[&id].alive {
        let data = capture_message(ctx, value, &transfers)?;
        shared.borrow().workers.workers[&id].tx.send(data).ok();
    }
    Ok(ctx.undefined())
}
pub(crate) fn ref_worker(
    ctx: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(ctx, receiver)?;
    let shared = ctx.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let worker = state.workers.workers.get_mut(&id).unwrap();
    worker.refed = true;
    worker.message_port_refed = true;
    Ok(ctx.undefined())
}
pub(crate) fn unref_worker(
    ctx: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(ctx, receiver)?;
    let shared = ctx.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let worker = state.workers.workers.get_mut(&id).unwrap();
    worker.refed = false;
    worker.message_port_refed = false;
    Ok(ctx.undefined())
}
pub(crate) fn handle_has_ref(
    ctx: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = ctx.host_function_data()?;
    let id = ctx
        .rooted_value(data)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("Worker handle ID missing"))? as u64;
    let value = {
        let shared = ctx.host_mut().shared_state();
        let state = shared.borrow();
        state
            .workers
            .workers
            .get(&id)
            .filter(|worker| worker.handle_active)
            .map(|worker| worker.refed)
    };
    match value {
        Some(refed) => Ok(ctx.boolean(refed)),
        None => Ok(ctx.undefined()),
    }
}
pub(crate) fn message_port_handle_has_ref(
    ctx: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = ctx.host_function_data()?;
    let id = ctx
        .rooted_value(data)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("MessagePort handle ID missing"))? as u64;
    let refed = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .workers
        .workers
        .get(&id)
        .filter(|worker| worker.handle_active)
        .is_some_and(|worker| worker.message_port_refed);
    Ok(ctx.boolean(refed))
}
pub(crate) fn terminate(
    ctx: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = arg(ctx, args, 0);
    if !ctx.rooted_value(callback).is_some_and(|v| v.is_undefined()) {
        return Err(fail(
            ctx,
            "Deprecated terminate callback is not implemented",
        )?);
    }
    let id = id(ctx, receiver)?;
    ctx.host_mut().shared_state().borrow().workers.workers[&id]
        .interrupt
        .request_termination();
    let exec = ctx.host_function_with_data(
        crate::host::shared_vm::operation("workerTerminateExecutor"),
        receiver,
    )?;
    let primordials = crate::host::shared_vm::commonjs::node_primordials(ctx)?
        .ok_or_else(|| RootedError::host("Worker requires pinned Node primordials"))?;
    let promise = get(ctx, primordials, "Promise")?;
    ctx.construct_rooted(promise, promise, &[exec])
}

fn cpu_usage_error(
    ctx: &mut Context<'_>,
    code: &'static str,
    message: &str,
    range: bool,
) -> Result<RootedError, RootedError> {
    let error = if range {
        ctx.range_error_rooted(message)?
    } else {
        ctx.type_error_rooted(message)?
    };
    if range {
        super::node_errors_shared_vm::decorate_range_error(ctx, error, code)?;
    } else {
        super::node_errors_shared_vm::decorate_type_error(ctx, error, code)?;
    }
    Ok(ctx.throw(error))
}

fn cpu_usage_baseline(
    ctx: &mut Context<'_>,
    args: &[RootId],
) -> Result<Option<(f64, f64)>, RootedError> {
    let value = arg(ctx, args, 0);
    match ctx.rooted_value(value) {
        Some(value) if value.is_undefined() || value.is_null() => Ok(None),
        Some(value) => match value.as_number() {
            Some(number) if number.is_nan() => Ok(None),
            Some(_) => Err(cpu_usage_error(
                ctx,
                "ERR_INVALID_ARG_TYPE",
                "The \"prevValue\" argument must be of type object",
                false,
            )?),
            None => {
                let object = value_root(ctx, args, 0);
                if !ctx.is_object_rooted(object)? {
                    return Err(cpu_usage_error(
                        ctx,
                        "ERR_INVALID_ARG_TYPE",
                        "The \"prevValue\" argument must be of type object",
                        false,
                    )?);
                }
                if ctx.is_array_rooted(object)? {
                    return Err(cpu_usage_error(
                        ctx,
                        "ERR_INVALID_ARG_TYPE",
                        "The \"prevValue\" argument must be of type object",
                        false,
                    )?);
                }
                let user = get(ctx, object, "user")?;
                let system = get(ctx, object, "system")?;
                let user = ctx.rooted_value(user).and_then(|value| value.as_number());
                let system = ctx.rooted_value(system).and_then(|value| value.as_number());
                let (Some(user), Some(system)) = (user, system) else {
                    return Err(cpu_usage_error(
                        ctx,
                        "ERR_INVALID_ARG_TYPE",
                        "The \"prevValue\" argument must have numeric user and system properties",
                        false,
                    )?);
                };
                if !user.is_finite() || user < 0.0 || !system.is_finite() || system < 0.0 {
                    return Err(cpu_usage_error(
                        ctx,
                        "ERR_OUT_OF_RANGE",
                        "The \"prevValue\" properties must be finite non-negative numbers",
                        true,
                    )?);
                }
                Ok(Some((user, system)))
            }
        },
        None => Err(RootedError::host("invalid cpuUsage argument root")),
    }
}

// Resolving an argument root is split out so no references into the VM heap
// survive while the NativeContext is mutably borrowed for property access.
fn value_root(ctx: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| ctx.undefined())
}

#[cfg(target_os = "linux")]
fn worker_cpu_ticks(tid: u64) -> Result<(f64, f64), RootedError> {
    let stat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat"))
        .map_err(|error| RootedError::host(error.to_string()))?;
    let fields = stat
        .rsplit_once(')')
        .map(|(_, fields)| fields)
        .ok_or_else(|| RootedError::host("invalid worker thread stat"))?;
    // The suffix starts at field 3 (state), so utime/stime (14/15) are
    // positions 11/12 respectively.
    let mut fields = fields.split_whitespace();
    let user = fields
        .nth(11)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| RootedError::host("invalid worker user CPU ticks"))?;
    let system = fields
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| RootedError::host("invalid worker system CPU ticks"))?;
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return Err(RootedError::host("sysconf(_SC_CLK_TCK) failed"));
    }
    let micros_per_tick = 1_000_000.0 / ticks_per_second as f64;
    Ok((
        user as f64 * micros_per_tick,
        system as f64 * micros_per_tick,
    ))
}

pub(crate) fn cpu_usage(
    ctx: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let baseline = cpu_usage_baseline(ctx, args)?;
    let worker_id = id(ctx, receiver)?;
    let (alive, tid) = {
        let shared = ctx.host_mut().shared_state();
        let shared = shared.borrow();
        let worker = shared
            .workers
            .workers
            .get(&worker_id)
            .ok_or_else(|| RootedError::host("Invalid Worker receiver"))?;
        (worker.alive, worker.cpu_tid.load(Ordering::Acquire))
    };
    let promise = ctx.pending_promise_rooted()?;
    if !alive {
        let error = ctx.error_rooted("Worker is not running")?;
        super::node_errors_shared_vm::decorate_error(ctx, error, "ERR_WORKER_NOT_RUNNING")?;
        ctx.reject_promise_rooted(promise, error)?;
        return Ok(promise);
    }
    #[cfg(target_os = "linux")]
    let usage = if tid == 0 {
        Err(RootedError::host("worker thread ID is not available yet"))
    } else {
        worker_cpu_ticks(tid)
    };
    #[cfg(not(target_os = "linux"))]
    let usage: Result<(f64, f64), RootedError> = Err(RootedError::host(
        "worker.cpuUsage is not supported on this platform",
    ));
    match usage {
        Ok((user, system)) => {
            let (user, system) = baseline
                .map(|(previous_user, previous_system)| {
                    (
                        (user - previous_user).max(0.0),
                        (system - previous_system).max(0.0),
                    )
                })
                .unwrap_or((user, system));
            let result = ctx.object_rooted()?;
            let user_value = ctx.number(user);
            let system_value = ctx.number(system);
            define(ctx, result, "user", user_value)?;
            define(ctx, result, "system", system_value)?;
            ctx.resolve_promise_rooted(promise, result)?;
        }
        Err(error) => {
            let reason = ctx.error_rooted(&error.to_string())?;
            super::node_errors_shared_vm::decorate_error(ctx, reason, "ERR_OPERATION_FAILED")?;
            ctx.reject_promise_rooted(promise, reason)?;
        }
    }
    Ok(promise)
}

pub(crate) fn terminate_executor(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let receiver = ctx.host_function_data()?;
    let id = id(ctx, receiver)?;
    let resolve = arg(ctx, args, 0);
    let shared = ctx.host_mut().shared_state();
    if shared.borrow().workers.workers[&id].alive {
        let resolve = ctx.retain(resolve)?;
        if shared.borrow().workers.workers[&id].root.is_none() {
            let root = ctx.retain(receiver)?;
            shared
                .borrow_mut()
                .workers
                .workers
                .get_mut(&id)
                .unwrap()
                .root = Some(root);
        }
        shared
            .borrow_mut()
            .workers
            .workers
            .get_mut(&id)
            .unwrap()
            .resolvers
            .push(resolve);
    } else {
        let u = ctx.undefined();
        ctx.call_rooted(resolve, u, &[u])?;
    }
    Ok(ctx.undefined())
}
pub(crate) fn port_post(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = arg(ctx, args, 0);
    let list = arg(ctx, args, 1);
    let transfers = transfers(ctx, list)?;
    let shared = ctx.host_mut().shared_state();
    if shared
        .borrow()
        .workers
        .child
        .as_ref()
        .is_some_and(|c| !c.closed)
    {
        let data = capture_message(ctx, value, &transfers)?;
        shared
            .borrow()
            .workers
            .child
            .as_ref()
            .unwrap()
            .tx
            .send(Event::Message(data))
            .ok();
    }
    Ok(ctx.undefined())
}

pub(crate) fn internal_post(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = arg(ctx, args, 0);
    let shared = ctx.host_mut().shared_state();
    if shared
        .borrow()
        .workers
        .child
        .as_ref()
        .is_some_and(|child| !child.closed)
    {
        let message = capture_message(ctx, value, &[])?;
        shared
            .borrow()
            .workers
            .child
            .as_ref()
            .unwrap()
            .tx
            .send(Event::InternalMessage(message))
            .ok();
    }
    Ok(ctx.undefined())
}

fn receive_broadcast(ctx: &mut Context<'_>, name: RootId, data: RootId) -> Result<(), RootedError> {
    let global = ctx.global_root()?;
    let receive = get(ctx, global, "__quenchBroadcastReceive")?;
    let undefined = ctx.undefined();
    ctx.call_rooted(receive, undefined, &[name, data])?;
    Ok(())
}

pub(crate) fn broadcast_post(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = arg(ctx, args, 0);
    let data = arg(ctx, args, 1);
    let envelope = ctx.object_rooted()?;
    let message_type = ctx.string_rooted("quenchBroadcastChannel");
    define(ctx, envelope, "type", message_type)?;
    define(ctx, envelope, "name", name)?;
    define(ctx, envelope, "data", data)?;
    let message = capture_message(ctx, envelope, &[])?;

    let shared = ctx.host_mut().shared_state();
    let (is_child, child_tx, worker_txs) = {
        let state = shared.borrow();
        let is_child = state.workers.thread_id != 0;
        let child_tx = if is_child {
            state
                .workers
                .child
                .as_ref()
                .filter(|child| !child.closed)
                .map(|child| child.tx.clone())
        } else {
            None
        };
        let worker_txs = if is_child {
            Vec::new()
        } else {
            state
                .workers
                .workers
                .values()
                .filter(|worker| worker.alive)
                .map(|worker| worker.tx.clone())
                .collect()
        };
        (is_child, child_tx, worker_txs)
    };
    if is_child {
        if let Some(tx) = child_tx {
            let _ = tx.send(Event::InternalMessage(message));
        }
    } else {
        for tx in worker_txs {
            let _ = tx.send(message.clone());
        }
    }
    Ok(ctx.undefined())
}

pub(crate) fn broadcast_ref(
    ctx: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = arg(ctx, args, 0);
    let refed = ctx.truthy_rooted(value)?;
    let shared = ctx.host_mut().shared_state();
    if let Some(child) = shared.borrow_mut().workers.child.as_mut() {
        if refed {
            child.broadcast_refs = child.broadcast_refs.saturating_add(1);
        } else {
            child.broadcast_refs = child.broadcast_refs.saturating_sub(1);
        }
    }
    Ok(ctx.undefined())
}

pub(crate) fn port_on(
    ctx: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let proto = ctx.prototype_rooted(receiver)?;
    let on = get(ctx, proto, "on")?;
    ctx.call_rooted(on, receiver, args)?;
    let event = arg(ctx, args, 0);
    if ctx.to_string(event)? == "message" {
        ctx.host_mut()
            .shared_state()
            .borrow_mut()
            .workers
            .child
            .as_mut()
            .unwrap()
            .listening = true;
        ctx.host_mut()
            .shared_state()
            .borrow_mut()
            .workers
            .child
            .as_mut()
            .unwrap()
            .refed = true;
    }
    Ok(receiver)
}
pub(crate) fn port_ref(
    ctx: &mut Context<'_>,
    _receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .child
        .as_mut()
        .unwrap()
        .refed = true;
    Ok(ctx.undefined())
}
pub(crate) fn port_unref(
    ctx: &mut Context<'_>,
    _receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .child
        .as_mut()
        .unwrap()
        .refed = false;
    Ok(ctx.undefined())
}
pub(crate) fn port_close(
    ctx: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = ctx.host_mut().shared_state();
    {
        let mut state = shared.borrow_mut();
        let child = state.workers.child.as_mut().unwrap();
        child.closed = true;
        child.listening = false;
    }
    emit(ctx, receiver, "close", &[])?;
    Ok(ctx.undefined())
}
pub(crate) fn port_has_ref(
    ctx: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let yes = ctx
        .host_mut()
        .shared_state()
        .borrow()
        .workers
        .child
        .as_ref()
        .is_some_and(|c| c.refed && !c.closed);
    Ok(ctx.boolean(yes))
}
fn refresh_port(ctx: &mut Context<'_>) -> Result<(), RootedError> {
    let shared = ctx.host_mut().shared_state();
    let Some(port) = shared.borrow().workers.port else {
        return Ok(());
    };
    let listeners = get(ctx, port, "_nodeListeners")?;
    let get_fn = get(ctx, listeners, "get")?;
    let name = ctx.string_rooted("message");
    let list = ctx.call_rooted(get_fn, listeners, &[name])?;
    let has_node_listener = if ctx.rooted_value(list).is_some_and(|v| v.is_undefined()) {
        false
    } else {
        let length = get(ctx, list, "length")?;
        ctx.to_number(length)? > 0.
    };
    let onmessage = get(ctx, port, "onmessage")?;
    let listening = has_node_listener || ctx.is_callable_rooted(onmessage)?;
    let mut state = shared.borrow_mut();
    let child = state.workers.child.as_mut().unwrap();
    if child.listening != listening {
        child.refed = listening;
    }
    child.listening = listening;
    Ok(())
}
pub(crate) fn has_work(state: &State) -> bool {
    state
        .workers
        .values()
        .any(|w| w.alive && (w.refed || !w.resolvers.is_empty()))
        || state
            .child
            .as_ref()
            .is_some_and(|c| (c.refed || c.broadcast_refs != 0) && !c.closed)
}
pub(crate) fn poll_native(
    ctx: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    refresh_port(ctx)?;
    let shared = ctx.host_mut().shared_state();
    let mut events = Vec::new();
    let mut messages = Vec::new();
    {
        let mut state = shared.borrow_mut();
        for (id, w) in &state.workers.workers {
            while let Ok(e) = w.rx.try_recv() {
                events.push((*id, e))
            }
        }
        state.workers.pending.extend(events.drain(..));
        if let Some(event) = state.workers.pending.pop_front() {
            events.push(event)
        }
        if state
            .workers
            .child
            .as_ref()
            .is_some_and(|c| c.interrupt.is_termination_requested())
        {
            if state.process_control.requested_exit_code().is_none() {
                state.process_control.request_really_exit(1);
            }
            let child = state.workers.child.as_mut().unwrap();
            child.closed = true;
            child.listening = false;
        }
        if let Some(child) = state.workers.child.as_mut() {
            if !child.closed {
                if child.listening {
                    messages.extend(child.pending_messages.drain(..));
                }
                while let Ok(message) = child.rx.try_recv() {
                    messages.push(message);
                }
            }
        }
    }
    let ran = !events.is_empty() || !messages.is_empty();
    for (id, event) in events {
        let root = { shared.borrow().workers.workers[&id].root };
        let Some(root) = root else { continue };
        match event {
            Event::Online => emit(ctx, root, "online", &[])?,
            Event::Message(m) => {
                let m = clone::decode(ctx, &m)?;
                emit(ctx, root, "message", &[m])?
            }
            Event::InternalMessage(message) => {
                let message_graph = message;
                let message = clone::decode(ctx, &message_graph)?;
                let message_type = get(ctx, message, "type")?;
                let message_type = ctx.to_string(message_type)?;
                if message_type == "quenchBroadcastChannel" {
                    let name = get(ctx, message, "name")?;
                    let data = get(ctx, message, "data")?;
                    receive_broadcast(ctx, name, data)?;
                    let worker_txs = {
                        let state = shared.borrow();
                        state
                            .workers
                            .workers
                            .iter()
                            .filter(|(worker_id, worker)| **worker_id != id && worker.alive)
                            .map(|(_, worker)| worker.tx.clone())
                            .collect::<Vec<_>>()
                    };
                    for tx in worker_txs {
                        let _ = tx.send(message_graph.clone());
                    }
                    continue;
                }
                if message_type == "couldNotSerializeError" {
                    let codes = crate::host::shared_vm::commonjs::node_library_export(
                        ctx,
                        "internal/worker.js",
                        "internal/errors.js",
                        "codes",
                    )?;
                    let constructor = get(ctx, codes, "ERR_WORKER_UNSERIALIZABLE_ERROR")?;
                    let error = ctx.construct_rooted(constructor, constructor, &[])?;
                    // Keep this internal protocol error's public code stable
                    // even if the SDK error-code constructor is a plain
                    // Error on a host build without Node's coded-error brand.
                    let code_key = ctx.string_rooted("code");
                    let code = ctx.get_property_rooted(error, code_key)?;
                    if ctx
                        .rooted_value(code)
                        .is_some_and(|value| value.is_undefined())
                    {
                        let code = ctx.string_rooted("ERR_WORKER_UNSERIALIZABLE_ERROR");
                        ctx.define_data_property_rooted(error, code_key, code, true, false, true)?;
                    }
                    emit(ctx, root, "error", &[error])?;
                    continue;
                }
                let error =
                    ctx.error_rooted(&format!("Unknown worker message type {message_type}"))?;
                return Err(ctx.throw(error));
            }
            Event::Stdout(text) => {
                let stdout = get(ctx, root, "stdout")?;
                if !ctx
                    .rooted_value(stdout)
                    .is_some_and(|value| value.is_null())
                {
                    let global = ctx.global_root()?;
                    let buffer = get(ctx, global, "Buffer")?;
                    let from = get(ctx, buffer, "from")?;
                    let text = ctx.string_rooted(&text);
                    let chunk = ctx.call_rooted(from, buffer, &[text])?;
                    let push = get(ctx, stdout, "push")?;
                    ctx.call_rooted(push, stdout, &[chunk])?;
                }
            }
            Event::Stderr(text) => {
                let stderr = get(ctx, root, "stderr")?;
                let global = ctx.global_root()?;
                let buffer = get(ctx, global, "Buffer")?;
                let from = get(ctx, buffer, "from")?;
                let text = ctx.string_rooted(&text);
                let chunk = ctx.call_rooted(from, buffer, &[text])?;
                emit(ctx, stderr, "data", &[chunk])?;
            }
            Event::Error(e) => {
                let error = if let Some(packet) = e.serialized {
                    error_packet::import(ctx, &packet)?
                } else {
                    let ctor = ctx.with_startup_intrinsics(|ctx| {
                        let global = ctx.global_root()?;
                        get(ctx, global, &e.name)
                    })?;
                    let message = ctx.string_rooted(&e.message);
                    ctx.construct_rooted(ctor, ctor, &[message])?
                };
                emit(ctx, root, "error", &[error])?
            }
            Event::Exit(code) => {
                let stdout = get(ctx, root, "stdout")?;
                if !ctx
                    .rooted_value(stdout)
                    .is_some_and(|value| value.is_null())
                {
                    let push = get(ctx, stdout, "push")?;
                    let null = ctx.null();
                    ctx.call_rooted(push, stdout, &[null])?;
                }
                let n = ctx.number(code as f64);
                let (resolvers, async_resource, message_port_async_resource) = {
                    let mut state = shared.borrow_mut();
                    let w = state.workers.workers.get_mut(&id).unwrap();
                    w.alive = false;
                    w.thread_name = None;
                    w.root = None;
                    (
                        std::mem::take(&mut w.resolvers),
                        w.async_resource,
                        w.message_port_async_resource.take(),
                    )
                };
                // Node drops message listeners once the worker can no longer
                // deliver messages. Keep exit listeners intact so they can
                // observe the final state while message/messageerror listener
                // counts are already zero inside the exit callback.
                let remove_all = get(ctx, root, "removeAllListeners")?;
                for name in ["message", "messageerror"] {
                    let name = ctx.string_rooted(name);
                    ctx.call_rooted(remove_all, root, &[name])?;
                }
                if let Some(resource) = message_port_async_resource {
                    let destroy =
                        super::async_hooks_shared_vm::emit_destroy_immediate(ctx, resource);
                    ctx.release_root(resource);
                    if let Err(error) = destroy {
                        return Err(error);
                    }
                }
                let mut completion = emit(ctx, root, "exit", &[n]);
                if let Some(resource) = async_resource {
                    if let Err(error) =
                        super::async_hooks_shared_vm::emit_destroy(ctx, resource, &[])
                    {
                        if completion.is_ok() {
                            completion = Err(error);
                        }
                    }
                    shared
                        .borrow_mut()
                        .workers
                        .workers
                        .get_mut(&id)
                        .unwrap()
                        .handle_active = false;
                    shared
                        .borrow_mut()
                        .workers
                        .workers
                        .get_mut(&id)
                        .unwrap()
                        .async_resource = None;
                    ctx.release_root(resource);
                }
                let u = ctx.undefined();
                for resolve in resolvers {
                    if completion.is_ok() {
                        completion = ctx.call_rooted(resolve, u, &[n]).map(|_| ());
                    }
                    ctx.release_root(resolve);
                }
                ctx.release_root(root);
                let join = shared
                    .borrow_mut()
                    .workers
                    .workers
                    .get_mut(&id)
                    .unwrap()
                    .join
                    .take();
                if let Some(join) = join {
                    join.join().ok();
                }
                completion?;
            }
        }
    }
    let port = shared.borrow().workers.port;
    if let Some(port) = port {
        for m in messages {
            let value = clone::decode(ctx, &m)?;
            let message_type = get(ctx, value, "type")?;
            if ctx.to_string(message_type)? == "quenchBroadcastChannel" {
                let name = get(ctx, value, "name")?;
                let data = get(ctx, value, "data")?;
                receive_broadcast(ctx, name, data)?;
                continue;
            }
            let listening = shared
                .borrow()
                .workers
                .child
                .as_ref()
                .is_some_and(|child| child.listening && !child.closed);
            if listening {
                emit(ctx, port, "message", &[value])?;
            } else if let Some(child) = shared.borrow_mut().workers.child.as_mut() {
                child.pending_messages.push_back(m);
            }
            refresh_port(ctx)?;
        }
    }
    Ok(ctx.boolean(ran))
}
pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let f = runtime
        .host_function(crate::host::shared_vm::operation("workerPoll"))
        .map_err(|e| e.to_string())?;
    let u = runtime.root(quench_runtime::Value::UNDEFINED);
    let r = runtime.call_rooted(f, u, &[]);
    runtime.release_root(f);
    runtime.release_root(u);
    let r = match r {
        Ok(r) => r,
        Err(error) => {
            super::process_shared_vm::route_callback_failure(runtime, program, error)?;
            return Ok(true);
        }
    };
    let ran = runtime
        .rooted_value(r)
        .is_some_and(|v| v == quench_runtime::Value::TRUE);
    runtime.release_root(r);
    Ok(ran)
}
impl State {
    pub(crate) fn reset_vm_roots(&mut self) {
        self.workers.clear();
        self.pending.clear();
        self.port = None;
        self.serialized_failure = None;
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.interrupt.request_termination();
        if self.join.as_ref().is_some_and(|j| j.is_finished()) {
            if let Some(j) = self.join.take() {
                j.join().ok();
            }
        }
    }
}

/// Transfer an owned bounded error packet before the terminal Worker exit.
/// No VM value or root crosses the OS-thread boundary.
pub(crate) fn report_fatal_error(ctx: &mut Context<'_>, error: RootId) -> Result<(), RootedError> {
    let packet = match error_packet::capture(ctx, error) {
        Ok(packet) => Some(packet),
        Err(error) => {
            if error.error.is_execution_terminated() {
                return Err(error);
            }
            if let Some(exception) = error.exception {
                ctx.release_root(exception);
            }
            None
        }
    };
    let shared = ctx.host_mut().shared_state();
    let state = shared.borrow();
    let Some(child) = &state.workers.child else {
        return Ok(());
    };
    let failure = Failure {
        exit_code: state
            .process_control
            .exit_code()
            .unwrap_or(WORKER_FAILURE_EXIT_CODE),
        serialized: packet,
        name: "Error".into(),
        message: "Could not serialize Worker error".into(),
    };
    child.tx.send(Event::Error(failure)).ok();
    Ok(())
}

pub(crate) fn runtime_is_child(runtime: &mut Runtime<NodeHost>) -> bool {
    runtime
        .host_mut()
        .shared_state()
        .borrow()
        .workers
        .child
        .is_some()
}
