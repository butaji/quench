//! `child_process`'s public surface: the ChildProcess class, its stdio streams, and the
//! `spawn`/`fork`/`*Sync` entry points, over the host's child process operations. The
//! operations own the OS process and its pipes; a ChildProcess projects their events
//! (`__quenchDispatch`) onto its streams and its own `exit`/`close` events.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

/// The child's host id and its timeout and abort subscriptions.
const CHILD_ID: &str = "__quenchChildId";
const TIMEOUT_TIMER: &str = "__quenchTimeout";
const ABORT_SIGNAL: &str = "__quenchAbortSignal";
const ABORT_HANDLER: &str = "__quenchAbortHandler";
/// The signal a kill, timeout or abort sends unless told otherwise.
const DEFAULT_KILL_SIGNAL: &str = "SIGTERM";
/// The first stdio index past stdin, stdout and stderr.
const FIRST_EXTRA_STDIO: usize = 3;
const STDIN: usize = 0;
const STDOUT: usize = 1;
const STDERR: usize = 2;
/// The shells `shell: true` selects.
const POSIX_SHELL: &str = "/bin/sh";
const ANDROID_SHELL: &str = "/system/bin/sh";
const WINDOWS_SHELL: &str = "cmd.exe";

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)?;
    Ok(())
}

fn hide(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined() || value.is_null())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn call_method(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}

fn operation(c: &mut Context<'_>, name: &str) -> Outcome {
    c.host_function(crate::host::shared_vm::operation(name))
}

/// Call the host operation `name` with `args`.
fn invoke(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Outcome {
    let function = operation(c, name)?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, args)
}

fn closure(c: &mut Context<'_>, name: &str, captures: &[RootId]) -> Outcome {
    let data = c.array_rooted(captures)?;
    c.host_function_with_data(crate::host::shared_vm::operation(name), data)
}

fn capture(c: &mut Context<'_>, index: usize) -> Outcome {
    let data = c.host_function_data()?;
    get(c, data, &index.to_string())
}

fn type_error(c: &mut Context<'_>, code: &'static str, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(error) => match super::node_errors_shared_vm::decorate_type_error(c, error, code) {
            Ok(()) => c.throw(error),
            Err(failure) => failure,
        },
        Err(failure) => failure,
    }
}

fn stream_class(c: &mut Context<'_>, name: &str) -> Outcome {
    let stream = crate::host::shared_vm::commonjs::stream_module(c)?;
    get(c, stream, name)
}

fn process(c: &mut Context<'_>) -> Outcome {
    let global = c.global_root()?;
    get(c, global, "process")
}

/// The public module object, built once with the classes it exports.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let emitter_api = c
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventEmitter configuration missing"))?;
    let emitter = get(c, emitter_api, "emitterCtor")?;
    let dispose_key = get(c, emitter_api, "disposeKey")?;
    let child_process = c.host_constructor_named_with_data(
        crate::host::shared_vm::operation!("childProcessClassConstruct"),
        "ChildProcess",
        emitter,
    )?;
    let prototype = get(c, child_process, "prototype")?;
    let emitter_prototype = get(c, emitter, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    c.set_prototype_rooted(child_process, emitter)?;
    let initialize = c.host_function(crate::host::shared_vm::operation!("childProcessInitialize"))?;
    let spawn_method = c.host_function_named_with_data(
        crate::host::shared_vm::operation!("childProcessPrototypeSpawn"),
        "spawn",
        initialize,
    )?;
    let key = c.string_rooted("spawn");
    c.define_data_property_rooted(prototype, key, spawn_method, true, false, true)?;
    for (name, operation) in [
        ("kill", "childProcessKill"),
        ("ref", "childProcessRef"),
        ("unref", "childProcessUnref"),
        ("__quenchDispatch", "childProcessDispatch"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    let dispose = c.host_function_named(crate::host::shared_vm::operation!("childProcessDisposeMethod"), "[Symbol.dispose]")?;
    c.define_data_property_rooted(prototype, dispose_key, dispose, true, false, true)?;

    let module = c.object_rooted()?;
    let fork_child = c.host_function_named(crate::host::shared_vm::operation!("childProcessForkChild"), "_forkChild")?;
    put(c, module, "_forkChild", fork_child)?;
    put(c, module, "ChildProcess", child_process)?;
    let spawn_data = c.object_rooted()?;
    put(c, spawn_data, "constructor", child_process)?;
    let native_spawn = c.host_function_with_data(crate::host::shared_vm::operation!("childProcessConstruct"), spawn_data)?;
    let spawn = c.host_function_named_with_data(crate::host::shared_vm::operation!("childProcessSpawnPublic"), "spawn", native_spawn)?;
    let global = c.global_root()?;
    let buffer = get(c, global, "Buffer")?;
    let exec = c.host_function_named_with_data(crate::host::shared_vm::operation!("exec"), "exec", module)?;
    put(c, module, "exec", exec)?;
    let exec_file = super::child_process_exec_shared_vm::function(c, spawn, buffer)?;
    put(c, module, "execFile", exec_file)?;
    let buffer_prototype = get(c, buffer, "prototype")?;
    let native_sync = c.host_function_with_data(crate::host::shared_vm::operation!("childProcessSpawnSync"), buffer_prototype)?;
    let spawn_sync = c.host_function_named_with_data(crate::host::shared_vm::operation!("childProcessSpawnSyncPublic"), "spawnSync", native_sync)?;
    let sync_data = c.array_rooted(&[spawn_sync])?;
    for (name, operation) in [("execFileSync", "childProcessExecFileSync"), ("execSync", "childProcessExecSync")] {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, sync_data)?;
        put(c, module, name, function)?;
    }
    let is_buffer = get(c, buffer, "isBuffer")?;
    let fork_plan = c.host_function_with_data(crate::host::shared_vm::operation!("childProcessForkPlan"), is_buffer)?;
    let fork_data = c.array_rooted(&[fork_plan, spawn])?;
    let fork = c.host_function_named_with_data(crate::host::shared_vm::operation!("childProcessFork"), "fork", fork_data)?;
    put(c, module, "fork", fork)?;
    put(c, module, "spawn", spawn)?;
    put(c, module, "spawnSync", spawn_sync)?;
    super::child_process_exec_shared_vm::install_promisified(c, &[exec, exec_file])?;
    Ok(module)
}

/// `new ChildProcess()`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let emitter = c.host_function_data()?;
    c.call_rooted(emitter, this, &[])?;
    let no = c.boolean(false);
    let null = c.null();
    put(c, this, "connected", no)?;
    put(c, this, "signalCode", null)?;
    put(c, this, "exitCode", null)?;
    put(c, this, "killed", no)?;
    put(c, this, "spawnfile", null)?;
    Ok(this)
}

/// A signal's number, as `convertToValidSignal` accepts it.
fn signal_number(c: &mut Context<'_>, signal: RootId) -> Result<RootId, RootedError> {
    let os = crate::host::shared_vm::commonjs::builtin_require(c, "os")?;
    let constants = get(c, os, "constants")?;
    let signals = get(c, constants, "signals")?;
    if let Some(number) = c.rooted_value(signal).and_then(|value| value.as_number()) {
        if number == 0.0 {
            return Ok(signal);
        }
        for key in c.own_keys_rooted(signals)? {
            let value = c.get_property_rooted(signals, key)?;
            if c.rooted_value(value).and_then(|value| value.as_number()) == Some(number) {
                return Ok(signal);
            }
        }
    } else if let Some(name) = c.string_text(signal)? {
        let value = get(c, signals, &name.to_uppercase())?;
        if !is_undefined(c, value) {
            return Ok(value);
        }
    }
    let shown = c.to_string(signal)?;
    Err(type_error(c, "ERR_UNKNOWN_SIGNAL", &format!("Unknown signal: {shown}")))
}

pub(crate) fn kill(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let signal = arg(c, args, 0);
    let signal = if is_undefined(c, signal) { c.string_rooted(DEFAULT_KILL_SIGNAL) } else { signal };
    let number = signal_number(c, signal)?;
    let handle = get(c, this, "_handle")?;
    if !c.truthy_rooted(handle)? {
        return Ok(c.boolean(false));
    }
    let pid = get(c, this, "pid")?;
    let process = process(c)?;
    match super::stream_shared_vm::attempt(call_method(c, process, "kill", &[pid, number]))? {
        Ok(result) => {
            let yes = c.boolean(true);
            put(c, this, "killed", yes)?;
            Ok(result)
        }
        Err(_) => Ok(c.boolean(false)),
    }
}

fn reference(c: &mut Context<'_>, this: RootId, referenced: bool) -> Outcome {
    let referenced = c.boolean(referenced);
    invoke(c, "childProcessReference", &[this, referenced])?;
    Ok(c.undefined())
}

pub(crate) fn reference_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, this, true)
}

pub(crate) fn unreference_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, this, false)
}

pub(crate) fn dispose_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    invoke(c, "childProcessDispose", &[this])?;
    Ok(c.undefined())
}

/// `chunk` as a Buffer over the same bytes.
fn as_buffer(c: &mut Context<'_>, view: RootId) -> Outcome {
    let global = c.global_root()?;
    let buffer = get(c, global, "Buffer")?;
    let store = get(c, view, "buffer")?;
    let offset = get(c, view, "byteOffset")?;
    let length = get(c, view, "byteLength")?;
    call_method(c, buffer, "from", &[store, offset, length])
}

fn stdio_stream(c: &mut Context<'_>, child: RootId, index: RootId) -> Outcome {
    let stdio = get(c, child, "stdio")?;
    if !c.truthy_rooted(stdio)? {
        return Ok(c.undefined());
    }
    let index = c.to_string(index)?;
    get(c, stdio, &index)
}

fn push(c: &mut Context<'_>, stream: RootId, chunk: RootId) -> Result<(), RootedError> {
    if c.truthy_rooted(stream)? {
        call_method(c, stream, "push", &[chunk])?;
    }
    Ok(())
}

/// `__quenchDispatch(type, value, value2)`: one host event for this child.
pub(crate) fn dispatch(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (kind, value, value2) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let kind = c.to_string(kind)?;
    let null = c.null();
    match kind.as_str() {
        "stdout" | "stderr" => {
            let stream = get(c, this, &kind)?;
            let chunk = as_buffer(c, value)?;
            push(c, stream, chunk)?;
        }
        "extraStdout" => {
            let stream = stdio_stream(c, this, value2)?;
            let chunk = as_buffer(c, value)?;
            push(c, stream, chunk)?;
        }
        "stdoutEnd" | "stderrEnd" => {
            let stream = get(c, this, kind.trim_end_matches("End"))?;
            push(c, stream, null)?;
        }
        "extraStdoutEnd" => {
            let stream = stdio_stream(c, this, value)?;
            push(c, stream, null)?;
        }
        "exit" => {
            invoke(c, "childProcessPrepareExit", &[this, value, value2])?;
            let timer = get(c, this, TIMEOUT_TIMER)?;
            if !is_nullish(c, timer) {
                let global = c.global_root()?;
                call_method(c, global, "clearTimeout", &[timer])?;
            }
            let signal = get(c, this, ABORT_SIGNAL)?;
            let handler = get(c, this, ABORT_HANDLER)?;
            if !is_nullish(c, signal) && !is_nullish(c, handler) {
                let event = c.string_rooted("abort");
                call_method(c, signal, "removeEventListener", &[event, handler])?;
            }
            let exit_code = get(c, this, "exitCode")?;
            let signal_code = get(c, this, "signalCode")?;
            let event = c.string_rooted("exit");
            call_method(c, this, "emit", &[event, exit_code, signal_code])?;
            let id = get(c, this, CHILD_ID)?;
            invoke(c, "childProcessFlushStdio", &[id])?;
        }
        "close" => {
            let event = c.string_rooted("close");
            call_method(c, this, "emit", &[event, value, value2])?;
        }
        _ => {}
    }
    Ok(c.undefined())
}

/// A readable child output that asks the host for data as it is read.
fn output(c: &mut Context<'_>, id: RootId, stdout: bool) -> Outcome {
    let readable = stream_class(c, "Readable")?;
    let options = c.object_rooted()?;
    let which = c.boolean(stdout);
    let read = closure(c, "childProcessOutputRead", &[id, which])?;
    put(c, options, "read", read)?;
    let stream = c.construct_rooted(readable, readable, &[options])?;
    let handle = c.object_rooted()?;
    let read_start = c.host_function_named(crate::host::shared_vm::operation!("streamNop"), "readStart")?;
    put(c, handle, "readStart", read_start)?;
    put(c, stream, "_handle", handle)?;
    let destroy = operation(c, "childProcessDestroyReceiver")?;
    let event = c.string_rooted("end");
    call_method(c, stream, "once", &[event, destroy])?;
    Ok(stream)
}

pub(crate) fn output_read(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let which = capture(c, 1)?;
    invoke(c, "childProcessRead", &[id, which])
}

pub(crate) fn destroy_receiver(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    call_method(c, this, "destroy", &[])
}

/// Complete a stream write or final through `callback` once the host has taken it.
fn complete(c: &mut Context<'_>, result: Result<RootId, RootedError>, callback: RootId) -> Outcome {
    let undefined = c.undefined();
    match super::stream_shared_vm::attempt(result)? {
        Ok(completed) => {
            if c.truthy_rooted(completed)? {
                c.call_rooted(callback, undefined, &[])?;
            }
        }
        Err(error) => {
            c.call_rooted(callback, undefined, &[error])?;
        }
    }
    Ok(undefined)
}

fn bytes_of(c: &mut Context<'_>, chunk: RootId) -> Outcome {
    if c.with_view_bytes_rooted(chunk, |_| ()).is_some() {
        return Ok(chunk);
    }
    let global = c.global_root()?;
    let buffer = get(c, global, "Buffer")?;
    call_method(c, buffer, "from", &[chunk])
}

/// The child's stdin: a writable-only stream into the host pipe.
fn input(c: &mut Context<'_>, id: RootId) -> Outcome {
    let writable = stream_class(c, "Writable")?;
    let options = c.object_rooted()?;
    for (name, operation) in [
        ("write", "childProcessInputWrite"),
        ("final", "childProcessInputFinal"),
        ("destroy", "childProcessInputDestroy"),
    ] {
        let function = closure(c, operation, &[id])?;
        put(c, options, name, function)?;
    }
    let stream = c.construct_rooted(writable, writable, &[options])?;
    let key = c.string_rooted("readable");
    let no = c.boolean(false);
    c.define_data_property_rooted(stream, key, no, true, false, true)?;
    Ok(stream)
}

pub(crate) fn input_write(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    let bytes = bytes_of(c, chunk)?;
    let result = invoke(c, "childProcessWrite", &[id, bytes, callback]);
    complete(c, result, callback)
}

pub(crate) fn input_final(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let callback = arg(c, args, 0);
    let result = invoke(c, "childProcessEnd", &[id, callback]);
    complete(c, result, callback)
}

pub(crate) fn input_destroy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    invoke(c, "childProcessDestroyInput", &[id, error, callback])
}

/// An extra pipe (`stdio[3+]`): a duplex over the host pipe at `index`.
fn extra(c: &mut Context<'_>, id: RootId, index: usize) -> Outcome {
    let duplex = stream_class(c, "Duplex")?;
    let options = c.object_rooted()?;
    let index = c.number(index as f64);
    for (name, operation) in [
        ("read", "childProcessExtraRead"),
        ("write", "childProcessExtraWrite"),
        ("final", "childProcessExtraFinal"),
    ] {
        let function = closure(c, operation, &[id, index])?;
        put(c, options, name, function)?;
    }
    c.construct_rooted(duplex, duplex, &[options])
}

pub(crate) fn extra_read(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let index = capture(c, 1)?;
    invoke(c, "childProcessReadExtra", &[id, index])
}

pub(crate) fn extra_write(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let index = capture(c, 1)?;
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    let bytes = bytes_of(c, chunk)?;
    let result = invoke(c, "childProcessWriteExtra", &[id, index, bytes, callback]);
    complete(c, result, callback)
}

pub(crate) fn extra_final(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let id = capture(c, 0)?;
    let index = capture(c, 1)?;
    let callback = arg(c, args, 0);
    let result = invoke(c, "childProcessEndExtra", &[id, index, callback]);
    complete(c, result, callback)
}

/// `initialize(file, args, started, options)` with the child as receiver: its streams,
/// its host registration, and its abort and timeout subscriptions.
pub(crate) fn initialize(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (spawn_args, started, options) = (arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let pid = get(c, started, "pid")?;
    let pid = if is_nullish(c, pid) { c.undefined() } else { pid };
    put(c, this, "pid", pid)?;
    let id = get(c, started, "id")?;
    hide(c, this, CHILD_ID, id)?;
    let piped = get(c, started, "piped")?;
    let piped = c.array_like_values_rooted(piped)?;
    let mut flags = Vec::with_capacity(piped.len());
    for flag in piped {
        flags.push(c.truthy_rooted(flag)?);
    }
    let null = c.null();
    let stdout = if flags.get(STDOUT) == Some(&true) { output(c, id, true)? } else { null };
    put(c, this, "stdout", stdout)?;
    let stderr = if flags.get(STDERR) == Some(&true) { output(c, id, false)? } else { null };
    put(c, this, "stderr", stderr)?;
    let stdin = if flags.get(STDIN) == Some(&true) { input(c, id)? } else { null };
    put(c, this, "stdin", stdin)?;
    let mut stdio = vec![stdin, stdout, stderr];
    for (index, piped) in flags.iter().enumerate().skip(FIRST_EXTRA_STDIO) {
        stdio.push(if *piped { extra(c, id, index)? } else { null });
    }
    let stdio = c.array_rooted(&stdio)?;
    put(c, this, "stdio", stdio)?;
    let error_code = get(c, started, "errorCode")?;
    let failed = c.truthy_rooted(error_code)?;
    if failed {
        invoke(c, "childProcessSpawnFailed", &[this, started, spawn_args])?;
    } else {
        invoke(c, "childProcessRegister", &[id, this, stdout, stderr, stdin])?;
        invoke(c, "childProcessSpawnSucceeded", &[this])?;
    }
    if !c.is_object_rooted(options)? {
        return Ok(c.undefined());
    }
    let kill_signal = get(c, options, "killSignal")?;
    let kill_signal = if c.truthy_rooted(kill_signal)? { kill_signal } else { c.string_rooted(DEFAULT_KILL_SIGNAL) };
    let signal = get(c, options, "signal")?;
    if c.truthy_rooted(signal)? {
        let abort = closure(c, "childProcessAbort", &[this, signal, kill_signal])?;
        hide(c, this, ABORT_SIGNAL, signal)?;
        hide(c, this, ABORT_HANDLER, abort)?;
        let aborted = get(c, signal, "aborted")?;
        if c.truthy_rooted(aborted)? {
            let undefined = c.undefined();
            c.call_rooted(abort, undefined, &[])?;
        } else {
            let event = c.string_rooted("abort");
            let once = c.object_rooted()?;
            let yes = c.boolean(true);
            put(c, once, "once", yes)?;
            call_method(c, signal, "addEventListener", &[event, abort, once])?;
        }
    }
    let timeout = get(c, options, "timeout")?;
    let timed = c.rooted_value(timeout).and_then(|value| value.as_number()).is_some_and(|ms| ms != 0.0);
    if timed && !failed {
        let expire = closure(c, "childProcessTimeout", &[this, kill_signal])?;
        let global = c.global_root()?;
        let timer = call_method(c, global, "setTimeout", &[expire, timeout])?;
        hide(c, this, TIMEOUT_TIMER, timer)?;
    }
    Ok(c.undefined())
}

pub(crate) fn abort(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let child = capture(c, 0)?;
    let signal = capture(c, 1)?;
    let kill_signal = capture(c, 2)?;
    let error = c.error_rooted("The operation was aborted")?;
    let name = c.string_rooted("AbortError");
    put(c, error, "name", name)?;
    let code = c.string_rooted("ABORT_ERR");
    put(c, error, "code", code)?;
    let reason = get(c, signal, "reason")?;
    if !is_undefined(c, reason) {
        put(c, error, "cause", reason)?;
    }
    let report = closure(c, "childProcessReportError", &[child, error])?;
    c.queue_microtask_rooted(report, &[])?;
    call_method(c, child, "kill", &[kill_signal])?;
    Ok(c.undefined())
}

pub(crate) fn report_error(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let child = capture(c, 0)?;
    let error = capture(c, 1)?;
    let event = c.string_rooted("error");
    call_method(c, child, "emit", &[event, error])?;
    Ok(c.undefined())
}

pub(crate) fn timeout(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let child = capture(c, 0)?;
    let kill_signal = capture(c, 1)?;
    let exit_code = get(c, child, "exitCode")?;
    let signal_code = get(c, child, "signalCode")?;
    let running = c.rooted_value(exit_code).is_some_and(|value| value.is_null())
        && c.rooted_value(signal_code).is_some_and(|value| value.is_null());
    if running {
        call_method(c, child, "kill", &[kill_signal])?;
    }
    Ok(c.undefined())
}

/// `args` as spawn's argument list, or the options when the list is omitted.
fn arguments_and_options(c: &mut Context<'_>, args: RootId, options: RootId, strict: bool) -> Result<(RootId, RootId), RootedError> {
    if is_nullish(c, args) {
        return Ok((c.array_rooted(&[])?, options));
    }
    if c.is_array_rooted(args)? {
        return Ok((args, options));
    }
    if strict {
        if !c.is_object_rooted(args)? {
            return Err(super::util_shared_vm::argument_type_error(c, args, "args", "Object")?);
        }
        if !is_undefined(c, options) {
            return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
        }
    }
    Ok((c.array_rooted(&[])?, args))
}

/// The shell `options.shell` names: itself, or the platform's default shell.
fn shell_path(c: &mut Context<'_>, shell: RootId) -> Result<String, RootedError> {
    if let Some(path) = c.string_text(shell)? {
        return Ok(path);
    }
    let process = process(c)?;
    let platform = get(c, process, "platform")?;
    Ok(match c.to_string(platform)?.as_str() {
        "win32" => {
            let env = get(c, process, "env")?;
            let comspec = get(c, env, "comspec")?;
            if c.truthy_rooted(comspec)? { c.to_string(comspec)? } else { WINDOWS_SHELL.to_owned() }
        }
        "android" => ANDROID_SHELL.to_owned(),
        _ => POSIX_SHELL.to_owned(),
    })
}

fn is_windows_command_shell(c: &mut Context<'_>, shell: &str) -> Result<bool, RootedError> {
    let process = process(c)?;
    let platform = get(c, process, "platform")?;
    if c.to_string(platform)? != "win32" {
        return Ok(false);
    }
    let name = shell.rsplit('\\').next().unwrap_or(shell).to_ascii_lowercase();
    Ok(name == "cmd" || name == "cmd.exe")
}

/// The file and arguments a shell runs `file args...` with.
fn through_shell(c: &mut Context<'_>, file: RootId, args: RootId, options: RootId) -> Result<(RootId, RootId), RootedError> {
    let shell = get(c, options, "shell")?;
    let shell = shell_path(c, shell)?;
    let file = c.to_string(file)?;
    let words = c.array_like_values_rooted(args)?;
    let mut parts = Vec::with_capacity(words.len());
    for word in &words {
        parts.push(c.to_string(*word)?);
    }
    invoke(c, "childProcessShellArgsWarning", &[args])?;
    let windows = is_windows_command_shell(c, &shell)?;
    let arguments: Vec<String> = if windows {
        let command = if parts.is_empty() { format!("\"{file}\"") } else { format!("\"{file}\" {}", parts.join(" ")) };
        let yes = c.boolean(true);
        put(c, options, "windowsVerbatimArguments", yes)?;
        ["/d", "/s", "/c"].iter().map(|flag| (*flag).to_owned()).chain(std::iter::once(command)).collect()
    } else {
        let mut command = vec![file];
        command.extend(parts);
        vec!["-c".to_owned(), command.join(" ")]
    };
    let arguments = arguments.iter().map(|text| c.string_rooted(text)).collect::<Vec<_>>();
    let arguments = c.array_rooted(&arguments)?;
    Ok((c.string_rooted(&shell), arguments))
}

/// `spawn(file, args, options)`.
pub(crate) fn spawn(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let native = c.host_function_data()?;
    let file = arg(c, args, 0);
    let file = invoke(c, "childProcessValidateFile", &[file])?;
    let (arguments, options) = {
        let (list, options) = (arg(c, args, 1), arg(c, args, 2));
        arguments_and_options(c, list, options, true)?
    };
    let no = c.boolean(false);
    let options = invoke(c, "childProcessPrepareOptions", &[options, no])?;
    let undefined = c.undefined();
    if c.is_object_rooted(options)? && {
        let shell = get(c, options, "shell")?;
        c.truthy_rooted(shell)?
    } {
        let (shell, shell_args) = through_shell(c, file, arguments, options)?;
        return c.call_rooted(native, undefined, &[shell, shell_args, options]);
    }
    c.call_rooted(native, undefined, &[file, arguments, options])
}

/// `spawnSync(file, args, options)`.
pub(crate) fn spawn_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let native = c.host_function_data()?;
    let command = arg(c, args, 0);
    let command = invoke(c, "childProcessValidateFile", &[command])?;
    let (arguments, options) = {
        let (list, options) = (arg(c, args, 1), arg(c, args, 2));
        arguments_and_options(c, list, options, false)?
    };
    for (index, word) in c.array_like_values_rooted(arguments)?.into_iter().enumerate() {
        if c.string_text(word)?.is_some_and(|text| text.contains('\0')) {
            return Err(type_error(
                c,
                "ERR_INVALID_ARG_VALUE",
                &format!("The argument 'args[{index}]' must be a string without null bytes."),
            ));
        }
    }
    let yes = c.boolean(true);
    let options = invoke(c, "childProcessPrepareOptions", &[options, yes])?;
    let options = if is_undefined(c, options) { c.null_object_rooted()? } else { options };
    for name in ["windowsHide", "windowsVerbatimArguments"] {
        let value = get(c, options, name)?;
        if is_undefined(c, value) {
            let no = c.boolean(false);
            put(c, options, name, no)?;
        }
    }
    let shell = get(c, options, "shell")?;
    let (file, arguments) = if c.truthy_rooted(shell)? {
        through_shell(c, command, arguments, options)?
    } else {
        (command, arguments)
    };
    let undefined = c.undefined();
    c.call_rooted(native, undefined, &[file, arguments, options])
}

/// `execSync`/`execFileSync`: spawnSync's output, or its failure as a thrown error.
fn synchronous(c: &mut Context<'_>, command: RootId, args: RootId, options: RootId, shell: bool) -> Outcome {
    let spawn_sync = capture(c, 0)?;
    let use_shell = c.boolean(shell);
    let options = invoke(c, "childProcessSyncOptions", &[options, use_shell])?;
    let undefined = c.undefined();
    let result = c.call_rooted(spawn_sync, undefined, &[command, args, options])?;
    let stdout = get(c, result, "stdout")?;
    let stderr = get(c, result, "stderr")?;
    let error = get(c, result, "error")?;
    let status = get(c, result, "status")?;
    let succeeded = c.rooted_value(status).and_then(|value| value.as_number()) == Some(0.0);
    if !c.truthy_rooted(error)? && succeeded {
        return Ok(stdout);
    }
    let error = if c.truthy_rooted(error)? {
        error
    } else {
        let command = c.to_string(command)?;
        c.error_rooted(&format!("Command failed: {command}"))?
    };
    put(c, error, "status", status)?;
    let signal = get(c, result, "signal")?;
    put(c, error, "signal", signal)?;
    put(c, error, "stdout", stdout)?;
    put(c, error, "stderr", stderr)?;
    Err(c.throw(error))
}

pub(crate) fn exec_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (command, options) = (arg(c, args, 0), arg(c, args, 1));
    let empty = c.array_rooted(&[])?;
    synchronous(c, command, empty, options, true)
}

pub(crate) fn exec_file_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (file, mut list, mut options) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if !c.is_array_rooted(list)? {
        options = list;
        list = c.array_rooted(&[])?;
    }
    synchronous(c, file, list, options, false)
}

/// `fork(modulePath, args, options)`: spawn the planned Node child.
pub(crate) fn fork(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let plan = capture(c, 0)?;
    let spawn = capture(c, 1)?;
    let module_path = arg(c, args, 0);
    let list = arg(c, args, 1);
    let list = if is_undefined(c, list) { c.array_rooted(&[])? } else { list };
    let options = arg(c, args, 2);
    let undefined = c.undefined();
    let plan = c.call_rooted(plan, undefined, &[module_path, list, options])?;
    let file = get(c, plan, "file")?;
    let arguments = get(c, plan, "args")?;
    let options = get(c, plan, "options")?;
    c.call_rooted(spawn, undefined, &[file, arguments, options])
}
