//! Genuine OS-thread execution. Each child constructs and owns its NodeHost + Vm.
//! Draft scope: eval workers, owned clone graph, owned transfer graphs and uncatchable Core termination.
use super::event_constructor_shared_vm::get;
use super::worker_error_packet::{self as error_packet, Packet};
use super::worker_owned_clone::{self as clone, Message};
use crate::host::NodeHost;
use quench_runtime::{
    HostWeakRoot, NativeContext, RootId, RootedError, Runtime,
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
                    destroy?;
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
    if !has_work(&runtime.host_mut().shared_state().borrow().workers) {
        return Ok(false);
    }
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

impl State {
    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        // The thread's own identity and launch configuration belong to the restoring process.
        let Self {
            thread_id: _, eval_script: _, thread_name: _, exec_argv: _, environment_data: _, inherited_expose_gc: _,
            serialized_failure, child, workers, pending, port,
        } = self;
        require_quiescent(serialized_failure.is_none() && child.is_none(), "a worker thread")?;
        require_quiescent(workers.is_empty() && pending.is_empty(), "a Worker")?;
        port.write(writer)
    }
    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        self.port = crate::host::startup_snapshot::HostSnap::read(reader)?;
        Ok(())
    }
}
