//! Child process execution backed by the host operating system.

use crate::host::{node_host::SharedChildProcess, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::{
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
#[cfg(not(unix))]
use std::io::Read;
use std::time::Duration;

static NEXT_CHILD_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(spawnSync, spawnNative, register, write, end, Buffer, warnShellArgs, forkPlan, requestRead, flushStdio) => {
    const { EventEmitter } = globalThis.process.getBuiltinModule("events");
    class ChildProcess extends EventEmitter {
      constructor(file, args, started, options) {
        super();
        const { Readable, Writable } = globalThis.process.getBuiltinModule("stream");
      this.pid = started.pid ?? undefined;
      this.spawnfile = file;
      this.spawnargs = [file, ...args];
      this.exitCode = null;
      this.signalCode = null;
      this.killed = false;
      this.connected = false;
      this.stdout = started.piped[1] ? new Readable({read(){requestRead(started.id,true);}}) : null;
      this.stderr = started.piped[2] ? new Readable({read(){requestRead(started.id,false);}}) : null;
        for (const stream of [this.stdout, this.stderr]) {
          stream?.once("end", () => stream.destroy());
        }
      const id = started.id;
      this.stdin = started.piped[0] ? new Writable({
        write(chunk, _encoding, callback) {
          let completed;
          try { completed=write(id,Buffer.from(chunk).toString("base64"),callback); }
          catch(error) { callback(error); return; }
          if(completed)callback();
        },
        final(callback) {
          let completed;
          try { completed=end(id,callback); }
          catch(error) { callback(error); return; }
          if(completed)callback();
        }
      }) : null;
      this.stdio = [this.stdin, this.stdout, this.stderr, ...started.piped.slice(3).map(() => null)];
      this.__quenchDispatch = (type, value, value2) => {
        if (type === "stdout") this.stdout?.push(Buffer.from(value, "base64"));
        else if (type === "stderr") this.stderr?.push(Buffer.from(value, "base64"));
        else if (type === "stdoutEnd") this.stdout?.push(null);
        else if (type === "stderrEnd") this.stderr?.push(null);
        else if (type === "exit") {
          this.exitCode = value;
          this.signalCode = value2;
          if (this.__quenchTimeout !== undefined) clearTimeout(this.__quenchTimeout);
          if (this.__quenchAbortSignal && this.__quenchAbortHandler) {
            this.__quenchAbortSignal.removeEventListener("abort", this.__quenchAbortHandler);
          }
          this.emit("exit", value, value2);
          flushStdio(id);
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
          this.stdout?.push(null);
          this.stderr?.push(null);
          this.emit("close", null, null);
        });
      } else {
        register(id, this, this.stdout, this.stderr, this.stdin);
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
    if (options?.shell) {
      const shell = typeof options.shell === "string"
        ? options.shell
        : (globalThis.process.platform === "win32" ? "cmd.exe" : "/bin/sh");
      const command = [file, ...args].join(" ");
      warnShellArgs(args);
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
      warnShellArgs(actualArgs);
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
  function fork(modulePath, args = [], options) {
    const plan = forkPlan(modulePath, args, options);
    return spawn(plan.file, plan.args, plan.options);
  }
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
  return { ChildProcess, spawn, spawnSync: spawnSyncFunction, fork, execSync, execFileSync };
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
    let warning = context.host_function(crate::host::shared_vm::operation(
        "childProcessShellArgsWarning",
    ))?;
    let key = context.string_rooted("isBuffer");
    let is_buffer = context.get_property_rooted(buffer, key)?;
    let fork_plan = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessForkPlan"),
        is_buffer,
    )?;
    let flush_stdio =
        context.host_function(crate::host::shared_vm::operation("childProcessFlushStdio"))?;
    let read = context.host_function(crate::host::shared_vm::operation("childProcessRead"))?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:child_process/shared.js")?;
    let undefined = context.undefined();
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            spawn_sync,
            spawn,
            register,
            write,
            end,
            buffer,
            warning,
            fork_plan,
            read,
            flush_stdio,
        ],
    )?;
    let key = context.string_rooted("spawn");
    let spawn = context.get_property_rooted(module, key)?;
    let exec_file = super::child_process_exec_shared_vm::function(context, spawn, buffer)?;
    let key = context.string_rooted("execFile");
    context.set_property_rooted(module, key, exec_file, module)?;
    let exec =
        context.host_function_with_data(crate::host::shared_vm::operation("exec"), module)?;
    let key = context.string_rooted("exec");
    context.set_property_rooted(module, key, exec, module)?;
    super::child_process_exec_shared_vm::install_promisified(context, &[exec, exec_file])?;
    Ok(module)
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
    command.args(&arguments);
    let stdio = super::child_process_stdio_shared_vm::normalize(context, options)?;
    let cwd = options
        .map(|root| property_cwd(context, root))
        .transpose()?
        .flatten();
    if let Some(env) = options
        .map(|root| property(context, root, "env"))
        .transpose()?
        .filter(|root| context.is_object_rooted(*root).unwrap_or(false))
    {
        super::child_process_env_shared_vm::configure(context, &mut command, env)?;
    }

    let configured = child_cwd(context, &mut command, cwd.as_deref())
        .and_then(|_| super::child_process_stdio_shared_vm::configure(&mut command, &stdio));
    let start = match configured
        .and_then(|configured| command.spawn().map(|child| (child, configured)))
    {
        Ok((mut child, configured)) => {
            if let Err(error) =
                super::child_process_stdio_shared_vm::activate_inherited_streams(context, &stdio)
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            let id = NEXT_CHILD_PROCESS_ID.fetch_add(1, Ordering::Relaxed);
            let pid = child.id();
            let ipc = configured.ipc;
            #[cfg(unix)]
            let [stdin_pipe, stdout_pipe, stderr_pipe] = configured.pipes;
            #[cfg(unix)]
            let stdin = super::child_process_input_shared_vm::Input::new(
                stdin_pipe.map(std::fs::File::from),
            );
            #[cfg(not(unix))]
            let stdin = super::child_process_input_shared_vm::Input::new(child.stdin.take());
            #[cfg(unix)]
            let stdout_pipe = super::child_process_output_shared_vm::Output::new(stdout_pipe);
            #[cfg(unix)]
            let stderr_pipe = super::child_process_output_shared_vm::Output::new(stderr_pipe);
            let (sender, receiver) = mpsc::channel();
            #[cfg(not(unix))]
            if let Some(stdout) = child.stdout.take() {
                spawn_reader(stdout, sender.clone(), true);
            }
            #[cfg(not(unix))]
            if let Some(stderr) = child.stderr.take() {
                spawn_reader(stderr, sender.clone(), false);
            }
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
                        #[cfg(unix)]
                        stdout_pipe,
                        #[cfg(unix)]
                        stderr_pipe,
                        stdin_stream: None,
                        stdin,
                        ipc,
                        events: receiver,
                        stdout_terminal: !stdio[1].piped(),
                        stderr_terminal: !stdio[2].piped(),
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
    let flags = stdio
        .iter()
        .map(|specification| context.boolean(specification.piped()))
        .collect::<Vec<_>>();
    let flags = context.array_rooted(&flags)?;
    set(context, result, "piped", flags)?;
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

#[cfg(not(unix))]
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
        #[cfg(unix)]
        Some(libc::EBADF) => "EBADF",
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
    let child = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("child process registration has no child"))?;
    let stdout = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("child process registration has no stdout"))?;
    let stderr = args
        .get(3)
        .copied()
        .ok_or_else(|| RootedError::host("child process registration has no stderr"))?;
    let stdin = args
        .get(4)
        .copied()
        .ok_or_else(|| RootedError::host("child process registration has no stdin"))?;
    let stdin = context.retain(stdin)?;
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
    process.stdin_stream = Some(stdin);
    process.child = Some(child);
    process.stdout = Some(stdout);
    process.stderr = Some(stderr);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .child_processes
        .insert(id, process);
    super::child_process_ipc_shared_vm::install(context, id, child)?;
    for (stream, is_stdout) in [(stdout, true), (stderr, false)] {
        if context
            .rooted_value(stream)
            .is_some_and(|value| value.is_null())
        {
            continue;
        }
        let id = context.number(id as f64);
        let is_stdout = context.boolean(is_stdout);
        let data = context.array_rooted(&[id, is_stdout])?;
        let closed = context.host_function_with_data(
            crate::host::shared_vm::operation("childProcessStreamClosed"),
            data,
        )?;
        let key = context.string_rooted("once");
        let once = context.get_property_rooted(stream, key)?;
        let event = context.string_rooted("close");
        context.call_rooted(once, stream, &[event, closed])?;
    }
    Ok(context.undefined())
}

/// Guest stream destruction closes our end of a child pipe independently of
/// EOF: a shell descendant may still hold its writer after the shell exits.
pub(crate) fn stream_closed(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let id_key = context.string_rooted("0");
    let id = context.get_property_rooted(data, id_key)?;
    let id = context
        .rooted_value(id)
        .and_then(quench_runtime::Value::as_number)
        .ok_or_else(|| RootedError::host("child stream close has no process id"))?
        as u64;
    let kind_key = context.string_rooted("1");
    let kind = context.get_property_rooted(data, kind_key)?;
    let is_stdout = context.truthy_rooted(kind)?;
    let state = context.host_mut().shared_state();
    if let Some(process) = state.borrow_mut().child_processes.get_mut(&id) {
        if is_stdout {
            process.stdout_terminal = true;
            #[cfg(unix)]
            process.stdout_pipe.close();
        } else {
            process.stderr_terminal = true;
            #[cfg(unix)]
            process.stderr_pipe.close();
        }
    }
    Ok(context.undefined())
}

pub(crate) fn queue_stdio_flush(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = context.host_function(crate::host::shared_vm::operation(
        "childProcessFlushStdioRun",
    ))?;
    let receiver = context.undefined();
    super::shared_event_loop::queue_next_tick(context, callback, receiver, args)?;
    Ok(context.undefined())
}

pub(crate) fn flush_stdio(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(quench_runtime::Value::as_number)
        .ok_or_else(|| RootedError::host("child stdio flush has no id"))? as u64;
    let state = context.host_mut().shared_state();
    let streams = {
        let state = state.borrow();
        state
            .child_processes
            .get(&id)
            .map(|process| {
                #[cfg(unix)]
                let streams = [
                    (!process.stdout_pipe.inherited())
                        .then_some(process.stdout)
                        .flatten(),
                    (!process.stderr_pipe.inherited())
                        .then_some(process.stderr)
                        .flatten(),
                ];
                #[cfg(not(unix))]
                let streams = [process.stdout, process.stderr];
                streams
            })
            .unwrap_or([None, None])
    };
    for stream in streams.into_iter().flatten() {
        if context
            .rooted_value(stream)
            .is_some_and(|value| value.is_null())
        {
            continue;
        }
        let key = context.string_rooted("resume");
        let resume = context.get_property_rooted(stream, key)?;
        context.call_rooted(resume, stream, &[])?;
    }
    Ok(context.undefined())
}

pub(crate) fn request_read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(unix)]
    {
        let id = args
            .first()
            .and_then(|root| context.rooted_value(*root))
            .and_then(quench_runtime::Value::as_number)
            .ok_or_else(|| RootedError::host("child output read has no id"))?
            as u64;
        let stdout = args
            .get(1)
            .copied()
            .map(|root| context.truthy_rooted(root))
            .transpose()?
            .unwrap_or(false);
        let state = context.host_mut().shared_state();
        if let Some(process) = state.borrow_mut().child_processes.get_mut(&id) {
            if stdout {
                process.stdout_pipe.request();
            } else {
                process.stderr_pipe.request();
            }
        };
    }
    #[cfg(not(unix))]
    let _ = args;
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
    let callback = args.get(2).copied()
        .ok_or_else(|| RootedError::host("child stdin write callback missing"))?;
    let callback = context.retain(callback)?;
    let state = context.host_mut().shared_state();
    let result = state.borrow_mut().child_processes.get_mut(&id).map_or_else(
        || Err(std::io::Error::from_raw_os_error(libc::EPIPE)),
        |process| process.stdin.write(bytes, callback),
    );
    match result {
        Ok(false) => Ok(context.boolean(false)),
        Ok(true) => {
            context.release_root(callback);
            Ok(context.boolean(true))
        }
        Err(error) => {
            context.release_root(callback);
            let error = super::child_process_input_shared_vm::write_error(
                context, error.raw_os_error().unwrap_or(libc::EIO),
            )?;
            Err(context.throw(error))
        }
    }
}

pub(crate) fn end_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(quench_runtime::Value::as_number)
        .ok_or_else(|| RootedError::host("child stdin end id missing"))? as u64;
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("child stdin final callback missing"))?;
    let callback = context.retain(callback)?;
    let state = context.host_mut().shared_state();
    let completed = state
        .borrow_mut()
        .child_processes
        .get_mut(&id)
        .is_none_or(|process| process.stdin.end(callback));
    if completed {
        context.release_root(callback);
    }
    Ok(context.boolean(completed))
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
        let (child, stdout, stderr, events, exit, close, input_progress) = {
            let mut state = shared_state.borrow_mut();
            let Some(process) = state.child_processes.get_mut(&id) else {
                continue;
            };
            let input_progress = process.stdin.progress();
            let mut events = Vec::new();
            loop {
                match process.events.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
                }
            }
            #[cfg(unix)]
            for (pipe, stdout) in [
                (&mut process.stdout_pipe, true),
                (&mut process.stderr_pipe, false),
            ] {
                use super::child_process_output_shared_vm::Progress;
                match pipe.progress() {
                    Progress::Idle => {},
                    Progress::Data(bytes) => events.push(if stdout { ChildProcessEvent::Stdout(bytes) } else { ChildProcessEvent::Stderr(bytes) }),
                    Progress::End => events.push(if stdout { ChildProcessEvent::StdoutEnd } else { ChildProcessEvent::StderrEnd }),
                }
            }
            let mut exit = None;
            for event in &events {
                match event {
                    ChildProcessEvent::Exit(code, signal) => {
                        process.exit = Some((*code, signal.clone()));
                        exit = Some((*code, signal.clone()));
                    }
                    _ => {}
                }
            }
            let close = process.stdout_terminal
                && process.stderr_terminal
                && process.exit.is_some()
                && process.ipc.as_ref().is_none_or(|channel| channel.closed())
                && !process.stdin.has_pending()
                && input_progress.callbacks.is_empty();
            let exit_for_close = process.exit.clone();
            (
                process.child,
                process.stdout,
                process.stderr,
                events,
                exit,
                close.then_some(exit_for_close).flatten(),
                input_progress,
            )
        };
        progressed |= input_progress.progressed;
        super::child_process_input_shared_vm::queue(runtime, program, input_progress.callbacks)?;
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
            if let Some(mut process) = process {
                drop(process.ipc.take());
                for root in [
                    process.child,
                    process.stdin_stream,
                    process.stdout,
                    process.stderr,
                ]
                .into_iter()
                .flatten()
                {
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
    #[cfg(unix)]
    { shared_state.borrow().child_processes.values().any(|process|
        process.exit.is_none() || process.stdin.has_pending()
        || process.stdout_pipe.is_referenced() || process.stderr_pipe.is_referenced()
    ) }
    #[cfg(not(unix))]
    {
        !shared_state.borrow().child_processes.is_empty()
    }
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
        .map(|root| property_cwd(context, root))
        .transpose()?
        .flatten();
    let max_buffer = if let Some(options) = options {
        let value = property(context, options, "maxBuffer")?;
        if context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            super::child_process_exec_shared_vm::DEFAULT_MAX_BUFFER
        } else {
            super::child_process_exec_shared_vm::validate_limit(
                context,
                value,
                "options.maxBuffer",
                false,
            )?;
            context
                .rooted_value(value)
                .and_then(quench_runtime::Value::as_number)
                // Node's synchronous binding treats an accepted null limit
                // as zero, its unlimited-buffer sentinel.
                .unwrap_or(0.0)
        }
    } else {
        super::child_process_exec_shared_vm::DEFAULT_MAX_BUFFER
    };

    let mut process = Command::new(&command);
    let argv0 = options
        .map(|root| property_string(context, root, "argv0"))
        .transpose()?
        .flatten();
    set_command_arg0(&mut process, argv0.as_deref());
    process.args(&arguments);
    process.stdout(Stdio::piped()).stderr(Stdio::piped());

    let env = options
        .map(|root| property(context, root, "env"))
        .transpose()?;
    if let Some(env) = env.filter(|root| context.is_object_rooted(*root).unwrap_or(false)) {
        super::child_process_env_shared_vm::configure(context, &mut process, env)?;
    }

    let output = match child_cwd(context, &mut process, cwd.as_deref()).and_then(|_| process.spawn()) {
        Ok(child) => {
            let pid = child.id();
            #[cfg(unix)]
            let collected = super::child_process_sync::collect(child, max_buffer)
                .map(|result| (result.output, result.limit_exceeded));
            #[cfg(not(unix))]
            let collected = {
                let _ = max_buffer;
                child.wait_with_output().map(|output| (output, false))
            };
            match collected {
                Ok((output, limit_exceeded)) => make_result(
                    context,
                    Some(pid),
                    if limit_exceeded {
                        None
                    } else {
                        output.status.code()
                    },
                    signal_name(&output.status),
                    &output.stdout,
                    &output.stderr,
                    limit_exceeded.then(|| ("ENOBUFS", format!("spawnSync {command} ENOBUFS"))),
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
    #[cfg(unix)]
    {
        let error = property(context, output, "error")?;
        if !context
            .rooted_value(error)
            .is_some_and(|value| value.is_null())
        {
            let code = property_string(context, error, "code")?.unwrap_or_default();
            let number = match code.as_str() {
                "ENOBUFS" => libc::ENOBUFS,
                "ENOENT" => libc::ENOENT,
                "EACCES" => libc::EACCES,
                "EIO" => libc::EIO,
                _ => libc::EINVAL,
            };
            let errno = context.number(f64::from(-number));
            set(context, error, "errno", errno)?;
            set_string(context, error, "syscall", &format!("spawnSync {command}"))?;
            set(context, error, "path", command_arg)?;
            let values = arguments
                .iter()
                .map(|argument| context.string_rooted(argument))
                .collect::<Vec<_>>();
            let spawnargs = context.array_rooted(&values)?;
            set(context, error, "spawnargs", spawnargs)?;
        }
    }
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
        let value = context.error_rooted(&message)?;
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

/// Resolve the OS launch boundary against the logical process directory after
/// guest argument/environment hooks finish. Keep kernel symlink resolution.
fn child_cwd(
    context: &mut NativeContext<'_, NodeHost>,
    command: &mut Command,
    requested: Option<&str>,
) -> std::io::Result<()> {
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
    cwd.configure_child_command(command, requested)
}

/// Node validates cwd as a filesystem path, including branded file URLs.
fn property_cwd(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<Option<String>, RootedError> {
    let value = property(context, options, "cwd")?;
    if context
        .rooted_value(value)
        .is_some_and(|v| v.is_undefined() || v.is_null())
    {
        return Ok(None);
    }
    if let Some(path) = context.string_text(value)? {
        reject_cwd_null(context, value, path.as_bytes().contains(&0))?;
        return Ok(Some(path));
    }
    if super::url_shared_vm::url_argument_text(context, value)?.is_some() {
        let undefined = context.undefined();
        let converted = super::url_shared_vm::file_url_to_path(context, undefined, &[value])?;
        let path = context
            .string_text(converted)?
            .ok_or_else(|| RootedError::host("file URL cwd is not a string"))?;
        reject_cwd_null(context, converted, path.as_bytes().contains(&0))?;
        return Ok(Some(path));
    }
    if context.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::Uint8Array)? {
        let bytes = context
            .view_bytes_rooted(value)
            .ok_or_else(|| RootedError::host("cwd byte view unavailable"))?;
        reject_cwd_null(context, value, bytes.contains(&0))?;
        // Node's child-process binding coerces a validated byte view to a
        // string; Buffer's UTF-8 conversion and Uint8Array's join both apply.
        let path = context.to_string(value)?;
        return Ok(Some(path));
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        value,
        "options.cwd",
        "a string or an instance of Buffer or URL",
    )?)
}

fn reject_cwd_null(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    contains_null: bool,
) -> Result<(), RootedError> {
    if !contains_null {
        return Ok(());
    }
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let undefined = context.undefined();
    let detail = context.call_rooted(inspect, undefined, &[value])?;
    let detail = context.to_string(detail)?;
    let error=context.type_error_rooted(&format!("The argument 'options.cwd' must be a string, Uint8Array, or URL without null bytes. Received {detail}"))?;
    let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
    set(context, error, "code", code)?;
    Err(context.throw(error))
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

/// normalizeSpawnArguments emits DEP0190 once across sync and async calls.
pub(crate) fn shell_args_warning(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let arguments = args.first().copied().unwrap_or(undefined);
    let key = context.string_rooted("length");
    let length = context.get_property_rooted(arguments, key)?;
    if !context.rooted_value(length).and_then(|value| value.as_number()).is_some_and(|length| length > 0.0) {
        return Ok(undefined);
    }
    let shared = context.host_mut().shared_state();
    if shared.borrow().child_process_shell_args_warning_emitted { return Ok(undefined); }
    let global = context.global_root()?;
    let key = context.string_rooted("process");
    let process = context.get_property_rooted(global, key)?;
    let key = context.string_rooted("emitWarning");
    let emit = context.get_property_rooted(process, key)?;
    let message = context.string_rooted("Passing args to a child process with shell option true can lead to security vulnerabilities, as the arguments are not escaped, only concatenated.");
    let kind = context.string_rooted("DeprecationWarning");
    let code = context.string_rooted("DEP0190");
    context.call_rooted(emit, process, &[message, kind, code])?;
    shared.borrow_mut().child_process_shell_args_warning_emitted = true;
    Ok(undefined)
}
