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
#[derive(Default)]
pub(crate) struct State {
    thread_id: u64,
    eval_script: Option<String>,
    serialized_failure: Option<Packet>,
    child: Option<Child>,
    workers: HashMap<u64, Worker>,
    pending: VecDeque<(u64, Event)>,
    port: Option<RootId>,
}
struct Child {
    data: Option<Message>,
    rx: mpsc::Receiver<Message>,
    tx: mpsc::Sender<Event>,
    interrupt: quench_runtime::ExecutionInterrupt,
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
    refed: bool,
    resolvers: Vec<RootId>,
}
struct Failure {
    serialized: Option<Packet>,
    name: String,
    message: String,
}
impl Failure {
    fn host(message: String) -> Self {
        Self {
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
    match packet {
        Some(packet) => Failure {
            serialized: Some(packet),
            name: String::new(),
            message: String::new(),
        },
        None => Failure::host(message),
    }
}
fn failure(runtime: &mut Runtime<NodeHost>, error: quench_runtime::JsError) -> Failure {
    let Some(value) = error.thrown_value() else {
        return completion_failure(runtime, error.to_string());
    };
    let root = runtime.root(value);
    record_callback_exception(runtime, root);
    runtime.release_root(root);
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
        ("terminate", "workerTerminate"),
    ] {
        method(ctx, proto, name, op)?
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
    define(ctx, module, "Worker", ctor)?;
    let shared = ctx.host_mut().shared_state();
    let thread_id = shared.borrow().workers.thread_id;
    let main = ctx.boolean(thread_id == 0);
    define(ctx, module, "isMainThread", main)?;
    let id = ctx.number(thread_id as f64);
    define(ctx, module, "threadId", id)?;
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
    let port = if thread_id == 0 {
        ctx.null()
    } else {
        let global = ctx.global_root()?;
        let port_ctor = get(ctx, global, "MessagePort")?;
        let port = ctx.construct_rooted(port_ctor, port_ctor, &[])?;
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
    for name in ["MessageChannel", "MessagePort"] {
        let v = get(ctx, global, name)?;
        define(ctx, module, name, v)?
    }
    Ok(module)
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
    let source = arg(ctx, args, 0);
    let options = arg(ctx, args, 1);
    let options = if ctx.rooted_value(options).is_some_and(|v| v.is_undefined()) {
        ctx.object_rooted()?
    } else {
        options
    };
    let (source, mode) = normalize_filename(ctx, source, options)?;
    let source = ctx.to_string(source)?;
    let eval = matches!(mode, FilenameMode::Classic);
    if matches!(mode, FilenameMode::DataUrl) {
        return Err(fail(ctx, "Data URL Workers are not implemented")?);
    }
    let entry_path = (!eval).then(|| std::path::PathBuf::from(&source));
    let list = get(ctx, options, "transferList")?;
    let transfers = constructor_transfers(ctx, list)?;
    let data = get(ctx, options, "workerData")?;
    // Reject explicitly rather than silently ignore environment/stdio/resource policy.
    for name in [
        "env",
        "stdin",
        "stdout",
        "stderr",
        "execArgv",
        "resourceLimits",
        "argv",
        "name",
        "trackUnmanagedFds",
    ] {
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
    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
    let interrupt = quench_runtime::ExecutionInterrupt::default();
    let child_interrupt = interrupt.clone();
    let (tx, rx) = mpsc::channel();
    let (up_tx, up_rx) = mpsc::channel();
    let library = ctx.host_mut().node_library_root.clone();
    let child_library = library;
    let argv = if eval {
        vec!["quench-worker".to_owned()]
    } else {
        vec!["quench-worker".to_owned(), source.clone()]
    };
    let join = thread::Builder::new()
        .name(format!("quench-worker-{thread_id}"))
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || {
            let report = up_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut host = NodeHost::new(argv);
                host.node_library_root = child_library;
                let (program_source, program_name, kind, eval_script) = if let Some(path) =
                    entry_path
                {
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
                    (
                        String::new(),
                        "[worker eval]".to_owned(),
                        quench_runtime::SourceKind::Script,
                        Some(source),
                    )
                };

                {
                    let mut state = host.shared_state.borrow_mut();
                    state.workers.thread_id = thread_id;
                    state.workers.eval_script = eval_script;
                    state.workers.child = Some(Child {
                        data: Some(data),
                        rx,
                        tx: up_tx,
                        interrupt: child_interrupt.clone(),
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
                        if let Some(root) = callback_exception.take() {
                            runtime.release_root(root);
                        }
                        if error.is_execution_terminated() {
                            if let Some(root) = exception {
                                runtime.release_root(root);
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
                            super::process_shared_vm::finish_requested_exit(&mut runtime, &program)
                                .map_err(|message| completion_failure(&mut runtime, message))
                        } else {
                            let handled = super::process_shared_vm::finish_after_uncaught_error(
                                &mut runtime,
                                &program,
                                &error,
                            );
                            match handled {
                                Ok(true) => Ok(()),
                                Ok(false) => Err(failure(&mut runtime, error)),
                                Err(message) => Err(completion_failure(&mut runtime, message)),
                            }
                        };
                        if let Some(root) = exception {
                            runtime.release_root(root);
                        }
                        if interrupt_handle.is_termination_requested() {
                            return Ok(1);
                        }
                        outcome?;
                        Ok(())
                    }
                };
                if interrupt_handle.is_termination_requested() {
                    return Ok(1);
                }
                completion.map_err(|message| completion_failure(&mut runtime, message))?;
                Ok::<_, Failure>(runtime.host_mut().exit_code().unwrap_or(0))
            }));
            match result {
                Ok(Ok(code)) => {
                    report.send(Event::Exit(code)).ok();
                }
                Ok(Err(e)) => {
                    report.send(Event::Error(e)).ok();
                    report.send(Event::Exit(1)).ok();
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
                refed: true,
                resolvers: Vec::new(),
            },
        );
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
    ctx.evaluate_script_rooted(&source, "[worker eval]")
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
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .workers
        .get_mut(&id)
        .unwrap()
        .refed = true;
    Ok(ctx.undefined())
}
pub(crate) fn unref_worker(
    ctx: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(ctx, receiver)?;
    ctx.host_mut()
        .shared_state()
        .borrow_mut()
        .workers
        .workers
        .get_mut(&id)
        .unwrap()
        .refed = false;
    Ok(ctx.undefined())
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
    let listening = if ctx.rooted_value(list).is_some_and(|v| v.is_undefined()) {
        false
    } else {
        let length = get(ctx, list, "length")?;
        ctx.to_number(length)? > 0.
    };
    let mut state = shared.borrow_mut();
    let child = state.workers.child.as_mut().unwrap();
    if child.listening && !listening {
        child.refed = false;
    }
    child.listening = listening;
    Ok(())
}
pub(crate) fn has_work(state: &State) -> bool {
    state
        .workers
        .values()
        .any(|w| w.alive && (w.refed || !w.resolvers.is_empty()))
        || state.child.as_ref().is_some_and(|c| c.refed && !c.closed)
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
            state.process_control.request_exit(1);
            let child = state.workers.child.as_mut().unwrap();
            child.closed = true;
            child.listening = false;
        }
        if let Some(child) = &state.workers.child {
            if child.listening && !child.closed {
                if let Ok(m) = child.rx.try_recv() {
                    messages.push(m)
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
                let n = ctx.number(code as f64);
                let resolvers = {
                    let mut state = shared.borrow_mut();
                    let w = state.workers.workers.get_mut(&id).unwrap();
                    w.alive = false;
                    w.root = None;
                    std::mem::take(&mut w.resolvers)
                };
                let mut completion = emit(ctx, root, "exit", &[n]);
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
            let m = clone::decode(ctx, &m)?;
            emit(ctx, port, "message", &[m])?;
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
