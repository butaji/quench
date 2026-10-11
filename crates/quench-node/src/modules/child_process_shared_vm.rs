//! Child process execution backed by the host operating system.

use crate::host::{node_host::SharedChildProcess, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
        Arc, Mutex,
    },
};
use std::time::Duration;

static NEXT_CHILD_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(spawnSync, spawnNative, register, write, end, Buffer) => {
  const spawn = (file, args, options) => {
    if (typeof file !== "string") {
      throw Object.assign(new TypeError('The "file" argument must be of type string.'), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    if (file.length === 0) {
      throw Object.assign(new TypeError('The "file" argument must be a non-empty string.'), {
        code: "ERR_INVALID_ARG_VALUE"
      });
    }
    if (args === undefined || args === null) {
      args = [];
    } else if (!Array.isArray(args)) {
      if (typeof args !== "object") {
        throw Object.assign(new TypeError('The "args" argument must be of type object.'), {
          code: "ERR_INVALID_ARG_TYPE"
        });
      }
      if (options !== undefined) {
        throw Object.assign(new TypeError('The "options" argument must be of type object.'), {
          code: "ERR_INVALID_ARG_TYPE"
        });
      }
      options = args;
      args = [];
    }
    if (options !== undefined &&
        (options === null || typeof options !== "object" || Array.isArray(options))) {
      throw Object.assign(new TypeError('The "options" argument must be of type object.'), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    if (options?.timeout !== undefined &&
        (typeof options.timeout !== "number" || !Number.isFinite(options.timeout) || options.timeout < 0)) {
      throw Object.assign(new RangeError('ERR_INVALID_ARG_TYPE: The value of "timeout" is out of range. It must be an unsigned integer.'), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    for (const name of ["uid", "gid"]) {
      if (options?.[name] !== undefined &&
          (!Number.isInteger(options[name]) || options[name] < 0 || options[name] > 0xFFFFFFFF)) {
        throw Object.assign(new RangeError(`The value of "options.${name}" is out of range.`), {
          code: "ERR_OUT_OF_RANGE"
        });
      }
    }
    const { EventEmitter } = globalThis.process.getBuiltinModule("events");
    class ChildProcess extends EventEmitter {
      constructor(file, args, started, options) {
        super();
        const { PassThrough, Writable } = globalThis.process.getBuiltinModule("stream");
      this.pid = started.pid ?? undefined;
      this.spawnfile = file;
      this.spawnargs = [file, ...args];
      this.exitCode = null;
      this.signalCode = null;
      this.killed = false;
      this.connected = false;
      this.stdout = new PassThrough();
      this.stderr = new PassThrough();
        for (const stream of [this.stdout, this.stderr]) {
          stream?.once("end", () => stream.destroy());
        }
      const id = started.id;
      this.stdin = new Writable({
        write(chunk, _encoding, callback) {
          try {
            write(id, Buffer.from(chunk).toString("base64"));
            callback();
          } catch (error) {
            callback(error);
          }
        },
        final(callback) {
          try {
            end(id);
            callback();
          } catch (error) {
            callback(error);
          }
        }
      });
      this.stdio = [this.stdin, this.stdout, this.stderr];
      this.__quenchDispatch = (type, value, value2) => {
        if (type === "stdout") this.stdout.push(Buffer.from(value, "base64"));
        else if (type === "stderr") this.stderr.push(Buffer.from(value, "base64"));
        else if (type === "stdoutEnd") this.stdout.push(null);
        else if (type === "stderrEnd") this.stderr.push(null);
        else if (type === "exit") {
          this.exitCode = value;
          this.signalCode = value2;
          if (this.__quenchTimeout !== undefined) clearTimeout(this.__quenchTimeout);
          if (this.__quenchAbortSignal && this.__quenchAbortHandler) {
            this.__quenchAbortSignal.removeEventListener("abort", this.__quenchAbortHandler);
          }
          this.emit("exit", value, value2);
        } else if (type === "close") {
          this.emit("close", value, value2);
        }
      };
      if (started.errorCode) {
        queueMicrotask(() => {
          const error = new Error(started.errorMessage);
          error.code = started.errorCode;
          error.errno = started.errorNumber;
          error.syscall = `spawn ${file}`;
          error.path = file;
          error.spawnargs = args;
          this.emit("error", error);
          this.stdout.push(null);
          this.stderr.push(null);
          this.emit("close", null, null);
        });
      } else {
        register(id, this, this.stdout, this.stderr);
        queueMicrotask(() => this.emit("spawn"));
      }
      const signal = options?.signal;
      if (signal) {
        const abort = () => {
          const error = new Error("The operation was aborted");
          error.name = "AbortError";
          error.code = "ABORT_ERR";
          if (signal.reason !== undefined) error.cause = signal.reason;
          queueMicrotask(() => this.emit("error", error));
          this.kill(options.killSignal || "SIGTERM");
        };
        this.__quenchAbortSignal = signal;
        this.__quenchAbortHandler = abort;
        if (signal.aborted) abort();
        else signal.addEventListener("abort", abort, { once: true });
      }
      const timeout = options?.timeout;
      if (timeout !== undefined && timeout !== 0 && !started.errorCode) {
        this.__quenchTimeout = setTimeout(() => {
          if (this.exitCode === null && this.signalCode === null) {
            this.kill(options.killSignal || "SIGTERM");
          }
        }, timeout);
      }
      }

      kill(signal = "SIGTERM") {
        if (this.exitCode !== null || this.signalCode !== null) return false;
        try {
          const result = globalThis.process.kill(this.pid, signal);
          this.killed = true;
          return result;
        } catch {
          return false;
        }
      }

      ref() { return this; }
      unref() { return this; }
    }
    if (options?.shell) {
      const shell = typeof options.shell === "string"
        ? options.shell
        : (globalThis.process.platform === "win32" ? "cmd.exe" : "/bin/sh");
      const command = [file, ...args].join(" ");
      if (args.length > 0 && options.shell === true) {
        globalThis.process.emitWarning(
          "Passing args to a child process with shell option true can lead to security vulnerabilities, as the arguments are not escaped, only concatenated.",
          { type: "DeprecationWarning", code: "DEP0190" }
        );
      }
      const shellArgs = globalThis.process.platform === "win32"
        ? ["/d", "/s", "/c", command]
        : ["-c", command];
      return new ChildProcess(shell, shellArgs, spawnNative(shell, shellArgs, options), options);
    }
    return new ChildProcess(file, args, spawnNative(file, args, options), options);
  };
  const spawnSyncFunction = (command, args, options) => {
    if (args === undefined || args === null) {
      args = [];
    } else if (!Array.isArray(args)) {
      options = args;
      args = [];
    }
    if (options !== undefined &&
        (options === null || typeof options !== "object" || Array.isArray(options))) {
      throw Object.assign(new TypeError('The "options" argument must be of type object.'), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    if (options?.argv0 !== undefined && typeof options.argv0 !== "string") {
      const received = Array.isArray(options.argv0) ? " Received an instance of Array" : "";
      throw Object.assign(new TypeError(`The "options.argv0" property must be of type string.${received}`), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    let file = command;
    let actualArgs = args || [];
    if (options?.shell) {
      const shell = typeof options.shell === "string"
        ? options.shell
        : (globalThis.process.platform === "win32" ? "cmd.exe" : "/bin/sh");
      const commandString = [command, ...actualArgs].join(" ");
      file = shell;
      actualArgs = globalThis.process.platform === "win32"
        ? ["/d", "/s", "/c", commandString]
        : ["-c", commandString];
      options = { ...options };
      delete options.shell;
    }
    const result = spawnSync(file, actualArgs, options);
    result.stdout = Buffer.from(result.stdout);
    result.stderr = Buffer.from(result.stderr);
    return result;
  };
  const parseFileArguments = (args, options, callback) => {
    if (typeof args === "function") {
      callback = args;
      args = [];
      options = {};
    } else if (args === undefined || args === null) {
      args = [];
    } else if (!Array.isArray(args)) {
      if (typeof args !== "object") {
        throw Object.assign(new TypeError('The "args" argument must be of type object.'), {
          code: "ERR_INVALID_ARG_TYPE"
        });
      }
      callback = options;
      options = args;
      args = [];
    }
    if (typeof options === "function") {
      callback = options;
      options = {};
    } else if (options === undefined || options === null) {
      options = {};
    }
    if (typeof options !== "object" || Array.isArray(options)) {
      throw Object.assign(new TypeError('The "options" argument must be of type object.'), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    if (callback !== undefined && callback !== null && typeof callback !== "function") {
      throw Object.assign(new TypeError('The "callback" argument must be of type function.'), {
        code: "ERR_INVALID_ARG_TYPE"
      });
    }
    return [args, options, callback];
  };
  const execFile = (file, args, options, callback) => {
    [args, options] = parseFileArguments(args, options, callback);
    return spawn(file, args, options);
  };
  const fork = (file, args, options) => {
    [args, options] = parseFileArguments(args, options);
    return spawn(globalThis.process.execPath, [file, ...args], options);
  };
  const exec = (command, options, callback) => {
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    options ||= {};
    const child = spawn(command, [], { ...options, shell: options.shell ?? true });
    if (typeof callback === "function") {
      const stdout = [];
      const stderr = [];
      child.stdout.on("data", (chunk) => stdout.push(Buffer.from(chunk)));
      child.stderr.on("data", (chunk) => stderr.push(Buffer.from(chunk)));
      child.on("error", (error) => callback(error, null, null));
      child.on("close", (code, signal) => {
        if (code === 0) {
          callback(null, Buffer.concat(stdout), Buffer.concat(stderr));
          return;
        }
        const error = new Error(`Command failed: ${command}`);
        error.code = code;
        error.signal = signal;
        error.stdout = Buffer.concat(stdout);
        error.stderr = Buffer.concat(stderr);
        callback(error, error.stdout, error.stderr);
      });
    }
    return child;
  };
  const synchronousCommand = (command, args, options, useShell) => {
    const result = spawnSyncFunction(command, args, { ...(options || {}), shell: useShell });
    const encoding = options?.encoding;
    const stdout = encoding && encoding !== "buffer" ? result.stdout.toString(encoding) : result.stdout;
    const stderr = encoding && encoding !== "buffer" ? result.stderr.toString(encoding) : result.stderr;
    if (result.error || result.status !== 0) {
      const error = result.error || new Error(`Command failed: ${command}`);
      error.status = result.status;
      error.signal = result.signal;
      error.stdout = stdout;
      error.stderr = stderr;
      throw error;
    }
    return stdout;
  };
  const execSync = (command, options) => synchronousCommand(command, [], options, true);
  const execFileSync = (file, args, options) => {
    if (!Array.isArray(args)) {
      options = args;
      args = [];
    }
    return synchronousCommand(file, args, options, false);
  };
  return { spawn, spawnSync: spawnSyncFunction, execFile, fork, exec, execSync, execFileSync };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, key)?;
    let spawn_sync = context.host_function(crate::host::shared_vm::operation(
        "childProcessSpawnSync",
    ))?;
    let spawn = context.host_function(crate::host::shared_vm::operation("childProcessSpawn"))?;
    let register = context.host_function(crate::host::shared_vm::operation("childProcessRegister"))?;
    let write = context.host_function(crate::host::shared_vm::operation("childProcessWrite"))?;
    let end = context.host_function(crate::host::shared_vm::operation("childProcessEnd"))?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:child_process/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[spawn_sync, spawn, register, write, end, buffer])
}

pub(crate) fn spawn_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(command_arg) = args.first().copied() else {
        let error = context.type_error_rooted("The \"file\" argument must be of type string")?;
        return Err(context.throw(error));
    };
    let file = context.to_string(command_arg)?;
    let arguments = args
        .get(1)
        .copied()
        .filter(|root| {
            !context
                .rooted_value(*root)
                .is_some_and(|value| value.is_undefined() || value.is_null())
        })
        .map(|root| string_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let options = args.get(2).copied().filter(|root| {
        !context
            .rooted_value(*root)
            .is_some_and(|value| value.is_undefined() || value.is_null())
    });
    let argv0 = options
        .map(|root| property_string(context, root, "argv0"))
        .transpose()?
        .flatten();

    let mut command = Command::new(&file);
    set_command_arg0(&mut command, argv0.as_deref());
    command
        .args(&arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = options
        .map(|root| property_string(context, root, "cwd"))
        .transpose()?
        .flatten()
    {
        command.current_dir(cwd);
    }
    if let Some(env) = options
        .map(|root| property(context, root, "env"))
        .transpose()?
        .filter(|root| context.is_object_rooted(*root).unwrap_or(false))
    {
        command.env_clear();
        for key in object_keys(context, env)? {
            let name = context.to_string(key)?;
            let value = property_string(context, env, &name)?.unwrap_or_default();
            command.env(name, value);
        }
    }

    let start = match command.spawn() {
        Ok(mut child) => {
            let id = NEXT_CHILD_PROCESS_ID.fetch_add(1, Ordering::Relaxed);
            let pid = child.id();
            let stdin = Arc::new(Mutex::new(child.stdin.take()));
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| RootedError::host("child process stdout pipe is missing"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| RootedError::host("child process stderr pipe is missing"))?;
            let (sender, receiver) = mpsc::channel();
            spawn_reader(stdout, sender.clone(), true);
            spawn_reader(stderr, sender.clone(), false);
            std::thread::Builder::new()
                .name("quench-child-wait".into())
                .spawn(move || {
                    let (code, signal) = match child.wait() {
                        Ok(status) => (status.code(), process_signal(&status)),
                        Err(_) => (None, None),
                    };
                    let _ = sender.send(crate::host::node_host::ChildProcessEvent::Exit(
                        code, signal,
                    ));
                })
                .map_err(|error| RootedError::host(error.to_string()))?;
            context
                .host_mut()
                .shared_state()
                .borrow_mut()
                .child_processes
                .insert(
                    id,
                    SharedChildProcess {
                        child: None,
                        stdout: None,
                        stderr: None,
                        stdin,
                        events: receiver,
                        stdout_ended: false,
                        stderr_ended: false,
                        exit: None,
                    },
                );
            (id, Some(pid), None, None, None)
        }
        Err(error) => (
            0,
            None,
            Some(spawn_error_code(error.raw_os_error())),
            Some(error.to_string()),
            error.raw_os_error().map(|number| -number),
        ),
    };

    let result = context.object_rooted()?;
    let id = context.number(start.0 as f64);
    set(context, result, "id", id)?;
    let pid = match start.1 {
        Some(pid) => context.number(pid as f64),
        None => context.null(),
    };
    set(context, result, "pid", pid)?;
    let error_code = match start.2.as_deref() {
        Some(code) => context.string_rooted(code),
        None => context.null(),
    };
    set(context, result, "errorCode", error_code)?;
    let error_message = match start.3.as_deref() {
        Some(message) => context.string_rooted(message),
        None => context.null(),
    };
    set(context, result, "errorMessage", error_message)?;
    let error_number = match start.4 {
        Some(number) => context.number(number as f64),
        None => context.null(),
    };
    set(context, result, "errorNumber", error_number)?;
    Ok(result)
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    sender: mpsc::Sender<crate::host::node_host::ChildProcessEvent>,
    stdout: bool,
) {
    std::thread::spawn(move || {
        let mut bytes = [0; 16 * 1024];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(length) => {
                    let event = if stdout {
                        crate::host::node_host::ChildProcessEvent::Stdout(
                            bytes[..length].to_vec(),
                        )
                    } else {
                        crate::host::node_host::ChildProcessEvent::Stderr(
                            bytes[..length].to_vec(),
                        )
                    };
                    if sender.send(event).is_err() {
                        return;
                    }
                }
            }
        }
        let event = if stdout {
            crate::host::node_host::ChildProcessEvent::StdoutEnd
        } else {
            crate::host::node_host::ChildProcessEvent::StderrEnd
        };
        let _ = sender.send(event);
    });
}

fn spawn_error_code(code: Option<i32>) -> &'static str {
    match code {
        Some(2) => "ENOENT",
        Some(13) => "EACCES",
        Some(8) => "ENOEXEC",
        _ => "UNKNOWN",
    }
}

#[cfg(unix)]
fn process_signal(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| match signal {
        2 => "SIGINT",
        9 => "SIGKILL",
        15 => "SIGTERM",
        _ => "SIGTERM",
    }.to_owned())
}

#[cfg(not(unix))]
fn process_signal(_: &std::process::ExitStatus) -> Option<String> {
    None
}

pub(crate) fn register_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = args.first().and_then(|root| {
        context
            .rooted_value(*root)
            .and_then(quench_runtime::Value::as_number)
            .map(|number| number as u64)
    }) else {
        return Err(RootedError::host("child process registration has no id"));
    };
    let child = args.get(1).copied().ok_or_else(|| RootedError::host("child process registration has no child"))?;
    let stdout = args.get(2).copied().ok_or_else(|| RootedError::host("child process registration has no stdout"))?;
    let stderr = args.get(3).copied().ok_or_else(|| RootedError::host("child process registration has no stderr"))?;
    let child = context.retain(child)?;
    let stdout = context.retain(stdout)?;
    let stderr = context.retain(stderr)?;
    let Some(mut process) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .child_processes
        .remove(&id)
    else {
        return Err(RootedError::host("child process registration id is unknown"));
    };
    process.child = Some(child);
    process.stdout = Some(stdout);
    process.stderr = Some(stderr);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .child_processes
        .insert(id, process);
    Ok(context.undefined())
}

pub(crate) fn write_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = args.first().and_then(|root| {
        context
            .rooted_value(*root)
            .and_then(quench_runtime::Value::as_number)
            .map(|number| number as u64)
    }) else {
        return Err(RootedError::host("child process write has no id"));
    };
    let encoded = args
        .get(1)
        .and_then(|root| context.string_text(*root).ok().flatten())
        .unwrap_or_default();
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let stdin = context
        .host_mut()
        .shared_state()
        .borrow()
        .child_processes
        .get(&id)
        .map(|process| Arc::clone(&process.stdin));
    if let Some(stdin) = stdin {
        if let Some(stdin) = stdin
            .lock()
            .map_err(|error| RootedError::host(error.to_string()))?
            .as_mut()
        {
            stdin
                .write_all(&bytes)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn end_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(id) = args.first().and_then(|root| {
        context
            .rooted_value(*root)
            .and_then(quench_runtime::Value::as_number)
            .map(|number| number as u64)
    }) else {
        return Err(RootedError::host("child process end has no id"));
    };
    if let Some(stdin) = context
        .host_mut()
        .shared_state()
        .borrow()
        .child_processes
        .get(&id)
        .map(|process| Arc::clone(&process.stdin))
    {
        stdin
            .lock()
            .map_err(|error| RootedError::host(error.to_string()))?
            .take();
    }
    Ok(context.undefined())
}

pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    shared_state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
) -> Result<bool, String> {
    use crate::host::node_host::ChildProcessEvent;
    let ids = shared_state
        .borrow()
        .child_processes
        .keys()
        .copied()
        .collect::<Vec<_>>();
    let mut progressed = false;
    for id in ids {
        let (child, stdout, stderr, events, exit, close) = {
            let mut state = shared_state.borrow_mut();
            let Some(process) = state.child_processes.get_mut(&id) else {
                continue;
            };
            let mut events = Vec::new();
            loop {
                match process.events.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
                }
            }
            let mut exit = None;
            for event in &events {
                match event {
                    ChildProcessEvent::StdoutEnd => process.stdout_ended = true,
                    ChildProcessEvent::StderrEnd => process.stderr_ended = true,
                    ChildProcessEvent::Exit(code, signal) => {
                        process.exit = Some((*code, signal.clone()));
                        exit = Some((*code, signal.clone()));
                    }
                    _ => {}
                }
            }
            let close = process.stdout_ended && process.stderr_ended && process.exit.is_some();
            let exit_for_close = process.exit.clone();
            (
                process.child,
                process.stdout,
                process.stderr,
                events,
                exit,
                close.then_some(exit_for_close).flatten(),
            )
        };
        let (Some(child), Some(_stdout), Some(_stderr)) = (child, stdout, stderr) else {
            continue;
        };
        for event in events {
            progressed = true;
            match event {
                ChildProcessEvent::Stdout(bytes) => {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    dispatch(runtime, program, child, "stdout", Some(&encoded), None)?;
                }
                ChildProcessEvent::Stderr(bytes) => {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    dispatch(runtime, program, child, "stderr", Some(&encoded), None)?;
                }
                ChildProcessEvent::StdoutEnd => {
                    dispatch(runtime, program, child, "stdoutEnd", None, None)?;
                }
                ChildProcessEvent::StderrEnd => {
                    dispatch(runtime, program, child, "stderrEnd", None, None)?;
                }
                ChildProcessEvent::Exit(_, _) => {
                    if let Some((code, signal)) = exit.as_ref() {
                        dispatch_exit(runtime, program, child, "exit", *code, signal.as_deref())?;
                    }
                }
            }
        }
        if let Some((code, signal)) = close {
            dispatch_exit(runtime, program, child, "close", code, signal.as_deref())?;
            let process = shared_state.borrow_mut().child_processes.remove(&id);
            if let Some(process) = process {
                for root in [process.child, process.stdout, process.stderr].into_iter().flatten() {
                    runtime.release_root(root);
                }
            }
            progressed = true;
        }
    }
    Ok(progressed)
}

fn dispatch(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    child: RootId,
    event: &str,
    value: Option<&str>,
    value2: Option<&str>,
) -> Result<(), String> {
    let event = runtime.string_rooted(event);
    let value = match value {
        Some(value) => runtime.string_rooted(value),
        None => runtime.root(quench_runtime::Value::NULL),
    };
    let value2 = match value2 {
        Some(value) => runtime.string_rooted(value),
        None => runtime.root(quench_runtime::Value::NULL),
    };
    let key = runtime.string_rooted("__quenchDispatch");
    let dispatch = runtime
        .get_property_rooted(child, key)
        .map_err(|error| runtime.format_error(program, &error.error))?;
    runtime.release_root(key);
    let result = runtime.call_rooted(dispatch, child, &[event, value, value2]);
    runtime.release_root(dispatch);
    runtime.release_root(event);
    runtime.release_root(value);
    runtime.release_root(value2);
    match result {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

fn dispatch_exit(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    child: RootId,
    event: &str,
    code: Option<i32>,
    signal: Option<&str>,
) -> Result<(), String> {
    let event = runtime.string_rooted(event);
    let code = match code {
        Some(code) => runtime.root(quench_runtime::Value::number(code as f64)),
        None => runtime.root(quench_runtime::Value::NULL),
    };
    let signal = match signal {
        Some(signal) => runtime.string_rooted(signal),
        None => runtime.root(quench_runtime::Value::NULL),
    };
    let key = runtime.string_rooted("__quenchDispatch");
    let dispatch = runtime
        .get_property_rooted(child, key)
        .map_err(|error| runtime.format_error(program, &error.error))?;
    runtime.release_root(key);
    let result = runtime.call_rooted(dispatch, child, &[event, code, signal]);
    runtime.release_root(dispatch);
    runtime.release_root(event);
    runtime.release_root(code);
    runtime.release_root(signal);
    match result {
        Ok(result) => {
            runtime.release_root(result);
            Ok(())
        }
        Err(error) => {
            let message = runtime.format_error(program, &error.error);
            if let Some(exception) = error.exception {
                runtime.release_root(exception);
            }
            Err(message)
        }
    }
}

pub(crate) fn has_pending(
    shared_state: &std::rc::Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
) -> bool {
    !shared_state.borrow().child_processes.is_empty()
}

pub(crate) const fn poll_interval() -> Duration {
    Duration::from_millis(1)
}

pub(crate) fn spawn_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(command_arg) = args.first().copied() else {
        let error = context.type_error_rooted("The \"file\" argument must be of type string")?;
        return Err(context.throw(error));
    };
    let command = context.to_string(command_arg)?;
    let arguments = args
        .get(1)
        .copied()
        .filter(|root| {
            !context
                .rooted_value(*root)
                .is_some_and(|value| value.is_undefined() || value.is_null())
        })
        .map(|root| string_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let options = args.get(2).copied().filter(|root| {
        !context
            .rooted_value(*root)
            .is_some_and(|value| value.is_undefined() || value.is_null())
    });
    let cwd = options
        .map(|root| property_string(context, root, "cwd"))
        .transpose()?
        .flatten();

    let mut process = Command::new(&command);
    let argv0 = options
        .map(|root| property_string(context, root, "argv0"))
        .transpose()?
        .flatten();
    set_command_arg0(&mut process, argv0.as_deref());
    process.args(arguments);
    process.stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }
    let env = options
        .map(|root| property(context, root, "env"))
        .transpose()?;
    if let Some(env) = env.filter(|root| context.is_object_rooted(*root).unwrap_or(false)) {
        process.env_clear();
        for key in object_keys(context, env)? {
            let name = context.to_string(key)?;
            let value = property_string(context, env, &name)?.unwrap_or_default();
            process.env(name, value);
        }
    }

    let output = match process.spawn() {
        Ok(child) => {
            let pid = child.id();
            match child.wait_with_output() {
                Ok(output) => make_result(
                    context,
                    Some(pid),
                    output.status.code(),
                    signal_name(&output.status),
                    &output.stdout,
                    &output.stderr,
                    None,
                )?,
                Err(error) => make_result(
                    context,
                    Some(pid),
                    None,
                    None,
                    &[],
                    &[],
                    Some(("EIO", error.to_string())),
                )?,
            }
        }
        Err(error) => {
            let code = error.raw_os_error().map_or("UNKNOWN", |code| match code {
                2 => "ENOENT",
                13 => "EACCES",
                _ => "UNKNOWN",
            });
            make_result(context, None, None, None, &[], &[], Some((code, error.to_string())))?
        }
    };
    Ok(output)
}

fn set_command_arg0(command: &mut Command, arg0: Option<&str>) {
    #[cfg(unix)]
    if let Some(arg0) = arg0 {
        use std::os::unix::process::CommandExt;
        command.arg0(arg0);
    }
    #[cfg(not(unix))]
    let _ = (command, arg0);
}

fn make_result(
    context: &mut NativeContext<'_, NodeHost>,
    pid: Option<u32>,
    status: Option<i32>,
    signal: Option<&str>,
    stdout: &[u8],
    stderr: &[u8],
    error: Option<(&str, String)>,
) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    let pid = match pid {
        Some(pid) => context.number(pid as f64),
        None => context.null(),
    };
    set(context, result, "pid", pid)?;
    let status = match status {
        Some(status) => context.number(status as f64),
        None => context.null(),
    };
    set(context, result, "status", status)?;
    let signal = match signal {
        Some(signal) => context.string_rooted(signal),
        None => context.null(),
    };
    set(context, result, "signal", signal)?;
    let stdout = bytes_array(context, stdout)?;
    let stderr = bytes_array(context, stderr)?;
    set(context, result, "stdout", stdout)?;
    set(context, result, "stderr", stderr)?;
    let error_value = if let Some((code, message)) = error {
        let value = context.object_rooted()?;
        set_string(context, value, "code", code)?;
        set_string(context, value, "message", &message)?;
        value
    } else {
        context.null()
    };
    set(context, result, "error", error_value)?;
    Ok(result)
}

fn bytes_array(context: &mut NativeContext<'_, NodeHost>, bytes: &[u8]) -> Result<RootId, RootedError> {
    let values = bytes
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn string_array(context: &mut NativeContext<'_, NodeHost>, array: RootId) -> Result<Vec<String>, RootedError> {
    let length = property(context, array, "length")?;
    let length = context.rooted_value(length).and_then(|value| value.as_number()).unwrap_or_default().max(0.0) as usize;
    (0..length)
        .map(|index| {
            let key = context.string_rooted(&index.to_string());
            let value = context.get_property_rooted(array, key)?;
            context.to_string(value)
        })
        .collect()
}

fn object_keys(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<Vec<RootId>, RootedError> {
    let global = context.global_root()?;
    let object = property(context, global, "Object")?;
    let keys = property(context, object, "keys")?;
    let array = context.call_rooted(keys, object, &[value])?;
    let length = property(context, array, "length")?;
    let length = context.rooted_value(length).and_then(|value| value.as_number()).unwrap_or_default().max(0.0) as usize;
    (0..length)
        .map(|index| {
            let key = context.string_rooted(&index.to_string());
            context.get_property_rooted(array, key)
        })
        .collect()
}

fn property_string(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = property(context, object, name)?;
    if context.rooted_value(value).is_some_and(|value| value.is_undefined() || value.is_null()) {
        Ok(None)
    } else {
        context.string_text(value)
    }
}

fn property(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set_string(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str, value: &str) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? { Ok(()) } else { Err(RootedError::host(format!("cannot set child_process result {name}"))) }
}

#[cfg(unix)]
fn signal_name(status: &std::process::ExitStatus) -> Option<&'static str> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| match signal { 9 => "SIGKILL", 15 => "SIGTERM", 2 => "SIGINT", _ => "SIGTERM" })
}

#[cfg(not(unix))]
fn signal_name(_: &std::process::ExitStatus) -> Option<&'static str> { None }
