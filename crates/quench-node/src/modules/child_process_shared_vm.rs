//! Child process execution backed by the host operating system.

use crate::host::{node_host::SharedChildProcess, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
#[cfg(not(unix))]
use std::io::Read;
use std::time::Duration;
use std::{
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

static NEXT_CHILD_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(spawnSync, spawnNative, register, write, end, Buffer, warnShellArgs, forkPlan, requestRead, flushStdio, prepareOptions, validateFile, syncOptions, disposeNative, disposeKey, referenceNative, destroyInput, prepareExit, spawnFailed, spawnSucceeded) => {
    const { EventEmitter } = globalThis.process.getBuiltinModule("events");
    class ChildProcess extends EventEmitter {
      constructor() {
        super();
        this.connected = false;
        this.signalCode = null;
        this.exitCode = null;
        this.killed = false;
        this.spawnfile = null;
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


    }
  function initialize(file, args, started, options) {
        const { Readable, Writable } = globalThis.process.getBuiltinModule("stream");
      this.pid = started.pid ?? undefined;
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
        },
        destroy(error, callback) { return destroyInput(id, error, callback); }
      }) : null;
      this.stdio = [this.stdin, this.stdout, this.stderr, ...started.piped.slice(3).map(() => null)];
      this.__quenchDispatch = (type, value, value2) => {
        if (type === "stdout") this.stdout?.push(Buffer.from(value, "base64"));
        else if (type === "stderr") this.stderr?.push(Buffer.from(value, "base64"));
        else if (type === "stdoutEnd") this.stdout?.push(null);
        else if (type === "stderrEnd") this.stderr?.push(null);
        else if (type === "exit") {
          prepareExit(this, value, value2);
          if (this.__quenchTimeout !== undefined) clearTimeout(this.__quenchTimeout);
          if (this.__quenchAbortSignal && this.__quenchAbortHandler) {
            this.__quenchAbortSignal.removeEventListener("abort", this.__quenchAbortHandler);
          }
          this.emit("exit", this.exitCode, this.signalCode);
          flushStdio(id);
        } else if (type === "close") {
          this.emit("close", value, value2);
        }
      };
      if (started.errorCode) {
        spawnFailed(this, started, args);
      } else {
        register(id, this, this.stdout, this.stderr, this.stdin);
        spawnSucceeded(this);
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
      if (timeout != null && timeout !== 0 && !started.errorCode) {
        this.__quenchTimeout = setTimeout(() => {
          if (this.exitCode === null && this.signalCode === null) {
            this.kill(options.killSignal || "SIGTERM");
          }
        }, timeout);
      }
  }
  ChildProcess.prototype[disposeKey] = function() { disposeNative(this); };
  ChildProcess.prototype.ref = function ref() { referenceNative(this, true); };
  ChildProcess.prototype.unref = function unref() { referenceNative(this, false); };
  const spawn = (file, args, options) => {
    file = validateFile(file);
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
    options = prepareOptions(options, false);
    if (options?.shell) {
      const shell = typeof options.shell === "string"
        ? options.shell
        : (globalThis.process.platform === "win32" ? "cmd.exe" : "/bin/sh");
      const command = [file, ...args].join(" ");
      warnShellArgs(args);
      const shellArgs = globalThis.process.platform === "win32"
        ? ["/d", "/s", "/c", command]
        : ["-c", command];
      return spawnNative(shell, shellArgs, options);
    }
    return spawnNative(file, args, options);
  };
  const spawnSyncFunction = (command, args, options) => {
    command = validateFile(command);
    if (args === undefined || args === null) {
      args = [];
    } else if (!Array.isArray(args)) {
      options = args;
      args = [];
    }
    options = prepareOptions(options, true);
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
    const result = spawnSyncFunction(command, args, syncOptions(options, useShell));
    const stdout = result.stdout;
    const stderr = result.stderr;
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
  return [{ ChildProcess, spawn, spawnSync: spawnSyncFunction, fork, execSync, execFileSync }, initialize];
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, key)?;
    let prototype = property(context, buffer, "prototype")?;
    let spawn_sync = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessSpawnSync"),
        prototype,
    )?;
    let spawn_data = context.object_rooted()?;
    let spawn = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessConstruct"), spawn_data,
    )?;
    let register =
        context.host_function(crate::host::shared_vm::operation("childProcessRegister"))?;
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
    let prepare_options = context.host_function(crate::host::shared_vm::operation(
        "childProcessPrepareOptions",
    ))?;
    let validate_file = context.host_function(crate::host::shared_vm::operation(
        "childProcessValidateFile",
    ))?;
    let sync_options =
        context.host_function(crate::host::shared_vm::operation("childProcessSyncOptions"))?;
    let dispose =
        context.host_function(crate::host::shared_vm::operation("childProcessDispose"))?;
    let operations = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("child disposal symbol owner missing"))?;
    let dispose_key = property(context, operations, "disposeKey")?;
    let reference =
        context.host_function(crate::host::shared_vm::operation("childProcessReference"))?;
    let destroy_input = context.host_function(crate::host::shared_vm::operation("childProcessDestroyInput"))?;
    let prepare_exit = context.host_function(crate::host::shared_vm::operation("childProcessPrepareExit"))?;
    let spawn_failed = context.host_function(
        crate::host::shared_vm::operation("childProcessSpawnFailed"),
    )?;
    let spawn_succeeded = context.host_function(
        crate::host::shared_vm::operation("childProcessSpawnSucceeded"),
    )?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:child_process/shared.js")?;
    let undefined = context.undefined();
    let factory_result = context.call_rooted(
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
            prepare_options,
            validate_file,
            sync_options,
            dispose,
            dispose_key,
            reference,
            destroy_input,
            prepare_exit,
            spawn_failed,
            spawn_succeeded,
        ],
    )?;
    let module = property(context, factory_result, "0")?;
    let initialize = property(context, factory_result, "1")?;
    let constructor = property(context, module, "ChildProcess")?;
    set(context, spawn_data, "constructor", constructor)?;
    let prototype = property(context, constructor, "prototype")?;
    let method = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessPrototypeSpawn"), initialize,
    )?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("spawn");
    context.define_data_property_rooted(method, key, name, false, false, true)?;
    let key = context.string_rooted("spawn");
    context.define_data_property_rooted(prototype, key, method, true, true, true)?;
    let spawn = context.get_property_rooted(module, key)?;
    let exec_file = super::child_process_exec_shared_vm::function(context, spawn, buffer)?;
    let key = context.string_rooted("execFile");
    context.set_property_rooted(module, key, exec_file, module)?;
    let exec =
        context.host_function_with_data(crate::host::shared_vm::operation("exec"), module)?;
    let key = context.string_rooted("exec");
    context.set_property_rooted(module, key, exec, module)?;
    super::child_process_exec_shared_vm::install_promisified(context, &[exec, exec_file])?;
    let initializer=context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessForkChildProjection"),module,
    )?;
    context.define_lazy_data_properties_rooted(module,&["_forkChild"],true,initializer)?;
    Ok(module)
}

pub(crate) fn spawn_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    spawn_child_with_stdio(context, args, None)
}

fn spawn_child_with_stdio(
    context: &mut NativeContext<'_, NodeHost>, args: &[RootId],
    stdio: Option<Vec<super::child_process_stdio_shared_vm::Specification>>,
) -> Result<RootId, RootedError> {
    let Some(command_arg) = args.first().copied() else {
        let error = context.type_error_rooted("The \"file\" argument must be of type string")?;
        return Err(context.throw(error));
    };
    let file = if stdio.is_some() {
        context.string_text(command_arg)?.ok_or_else(|| RootedError::host("validated child file missing"))?
    } else { super::child_process_options_shared_vm::file_text(context, command_arg)? };
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
    let stdio = match stdio {
        Some(stdio) => stdio,
        None => super::child_process_stdio_shared_vm::normalize(context, options)?,
    };
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

    let serialization = options
        .map(|root| property_string(context, root, "serialization"))
        .transpose()?
        .flatten()
        .and_then(|mode| super::child_process_ipc_shared_vm::Serialization::from_mode(&mode))
        .unwrap_or_default();
    let identity =
        super::child_process_options_shared_vm::Identity::from_options(context, options)?;
    let configured = child_cwd(context, &mut command, cwd.as_deref()).and_then(|_| {
        super::child_process_stdio_shared_vm::configure(&mut command, &stdio, serialization)
    });
    identity.configure(&mut command);
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
            // Keep cancellation ownership if OS wait-thread creation fails.
            let wait_owner = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
            let thread_owner = wait_owner.clone();
            let (cancel, cancellation) = Cancellation::new(pid);
            let (reap, reaping) = mpsc::channel::<()>();
            let wake = crate::modules::napi::thread_wake().ok();
            let wait_thread = std::thread::Builder::new()
                .name("quench-child-wait".into())
                .spawn(move || {
                    let mut child = thread_owner.lock().expect("child ownership poisoned")
                        .take().expect("child waiter owns process");
                    // Linux pidfd cancellation preserves identity while wait blocks.
                    // Fallback serializes cancellation with the sole Child owner.
                    let status = if let Some(cancellation) = cancellation { loop {
                        match child.try_wait() {
                            Ok(Some(status)) => break Some(status),
                            Err(_) => break None,
                            Ok(None) => {}
                        }
                        match cancellation.recv_timeout(poll_interval()) {
                            Ok(()) => {
                                let _ = child.kill();
                                break child.wait().ok();
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                // Normal removal follows observed exit. If initialization
                                // failed first, still stop/reap the actual process.
                                let _ = child.kill();
                                break child.wait().ok();
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                    }
                    } else {
                        // Observe the exit leaving the child unreaped: as under Node, a
                        // signal still reaches it until the loop delivers its exit, which
                        // drops `reap` and lets this waiter reap it.
                        match unreaped_exit_status(pid) {
                            Some(status) => {
                                send_exit(&sender, Some(status));
                                if let Some(wake) = &wake { crate::modules::napi::notify_wake(wake); }
                                let _ = reaping.recv();
                                let _ = child.wait();
                                return;
                            }
                            None => child.wait().ok(),
                        }
                    };
                    send_exit(&sender, status);
                    if let Some(wake) = &wake { crate::modules::napi::notify_wake(wake); }
                })
                ;
            if let Err(error) = wait_thread {
                if let Some(mut child) = wait_owner.lock().expect("child ownership poisoned").take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(RootedError::host(error.to_string()));
            }
            context
                .host_mut()
                .shared_state()
                .borrow_mut()
                .child_processes
                .insert(
                    id,
                    SharedChildProcess {
                        cancel,
                        referenced: true,
                        close_dispatched: false,
                        handle_phase: crate::host::node_host::ChildHandlePhase::Open,
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
                        ipc_pipe: None,
                        events: receiver,
                        reap: Some(reap),
                        stdout_terminal: !stdio[1].piped(),
                        stderr_terminal: !stdio[2].piped(),
                        exit: None,
                    },
                );
            (id, Some(pid), None, None, None)
        }
        Err(error) => {
            let code = spawn_error_code(error.raw_os_error());
            #[cfg(unix)]
            if !error.raw_os_error().is_some_and(|number| {
                matches!(
                    number,
                    libc::EACCES | libc::EAGAIN | libc::EMFILE | libc::ENFILE | libc::ENOENT
                )
            }) {
                let exception = context.error_rooted(&format!("spawn {code}"))?;
                if let Some(errno) = error.raw_os_error() {
                    let errno = context.number(-f64::from(errno));
                    set(context, exception, "errno", errno)?;
                }
                set_string(context, exception, "code", &code)?;
                set_string(context, exception, "syscall", "spawn")?;
                return Err(context.throw(exception));
            }
            (
                0,
                None,
                Some(code.clone()),
                Some(format!("spawn {file} {code}")),
                error.raw_os_error().map(|number| -number),
            )
        }
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
                        crate::host::node_host::ChildProcessEvent::Stdout(bytes[..length].to_vec())
                    } else {
                        crate::host::node_host::ChildProcessEvent::Stderr(bytes[..length].to_vec())
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

fn spawn_error_code(code: Option<i32>) -> String {
    code.map_or_else(
        || "UNKNOWN".to_owned(),
        |code| super::util_system_error_shared_vm::system_error_name(-f64::from(code)),
    )
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
        return Err(RootedError::host(
            "child process registration id is unknown",
        ));
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
    install_process_handle(context, id, child)?;
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
    let callback = args
        .get(2)
        .copied()
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
                context,
                error.raw_os_error().unwrap_or(libc::EIO),
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

/// Node closes the child's input at exit, before notifying exit observers.
/// The stream owns its destruction callback and public hook dispatch.
pub(crate) fn prepare_exit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let child = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("child exit receiver missing"))?;
    let code = args.get(1).copied().unwrap_or_else(|| context.null());
    let signal = args.get(2).copied().unwrap_or_else(|| context.null());
    if context.truthy_rooted(signal)? {
        context.assign_named_property_rooted(child, "signalCode", signal)?;
    } else {
        context.assign_named_property_rooted(child, "exitCode", code)?;
    }
    let stdin = property(context, child, "stdin")?;
    if context.truthy_rooted(stdin)? {
        let destroy = property(context, stdin, "destroy")?;
        context.call_rooted(destroy, stdin, &[])?;
    }
    // The poller has already moved the native process handle to Closing.
    // Clear its guest projection after input destruction, as Node does.
    let null = context.null();
    context.assign_named_property_rooted(child, "_handle", null)?;
    Ok(context.undefined())
}

pub(crate) fn destroy_input(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(quench_runtime::Value::as_number)
        .ok_or_else(|| RootedError::host("child stdin destroy id missing"))? as u64;
    if let Some(process) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .child_processes
        .get_mut(&id)
    {
        process.stdin.close();
    }
    let error = args.get(1).copied().unwrap_or_else(|| context.null());
    let callback = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("child stdin destroy callback missing"))?;
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, &[error])?;
    Ok(undefined)
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
        let raw_ipc_closed = {
            let state=shared_state.borrow();
            state.child_processes.get(&id).and_then(|p|p.ipc_pipe)
                .is_none_or(|pipe|super::pipe_binding::is_closed(&state,pipe))
        };
        let (child, stdout, stderr, events, exit, close, input_progress) = {
            let mut state = shared_state.borrow_mut();
            let Some(process) = state.child_processes.get_mut(&id) else {
                continue;
            };
            if process.handle_phase == crate::host::node_host::ChildHandlePhase::Closing {
                process.handle_phase = crate::host::node_host::ChildHandlePhase::Closed;
            }
            let input_progress = process.stdin.progress();
            // The waiter's events are taken before the pipes are read, so output written
            // before an observed exit is read too, and reaches its streams first, as the pipes'
            // readiness precedes the exit notification in Node.
            let mut observed = Vec::new();
            loop {
                match process.events.try_recv() {
                    Ok(event) => observed.push(event),
                    Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
                }
            }
            let mut events = Vec::new();
            #[cfg(unix)]
            for (pipe, stdout) in [
                (&mut process.stdout_pipe, true),
                (&mut process.stderr_pipe, false),
            ] {
                use super::child_process_output_shared_vm::Progress;
                match pipe.progress() {
                    Progress::Idle => {}
                    Progress::Data(bytes) => events.push(if stdout {
                        ChildProcessEvent::Stdout(bytes)
                    } else {
                        ChildProcessEvent::Stderr(bytes)
                    }),
                    Progress::End => events.push(if stdout {
                        ChildProcessEvent::StdoutEnd
                    } else {
                        ChildProcessEvent::StderrEnd
                    }),
                }
            }
            events.append(&mut observed);
            let mut exit = None;
            for event in &events {
                match event {
                    ChildProcessEvent::Exit(code, signal) => {
                        process.handle_phase = crate::host::node_host::ChildHandlePhase::Closing;
                        process.exit = Some((*code, signal.clone()));
                        exit = Some((*code, signal.clone()));
                    }
                    _ => {}
                }
            }
            let close = raw_ipc_closed && !process.close_dispatched
                && process.stdout_terminal
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
                    // The loop delivers the exit: the waiter may reap the child now.
                    if let Some(process) = shared_state.borrow_mut().child_processes.get_mut(&id) {
                        process.reap = None;
                    }
                    if let Some((code, signal)) = exit.as_ref() {
                        dispatch_exit(runtime, program, child, "exit", *code, signal.as_deref())?;
                    }
                }
            }
        }
        if let Some((code, signal)) = close {
            if let Some(process) = shared_state.borrow_mut().child_processes.get_mut(&id) {
                process.close_dispatched = true;
            }
            dispatch_exit(runtime, program, child, "close", code, signal.as_deref())?;
            progressed = true;
        }
        let finished = shared_state
            .borrow()
            .child_processes
            .get(&id)
            .is_some_and(|process| {
                process.close_dispatched
                    && process.handle_phase == crate::host::node_host::ChildHandlePhase::Closed
            });
        if finished {
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
    {
        shared_state
            .borrow()
            .child_processes
            .values()
            .any(|process| {
                (process.referenced
                    && process.handle_phase == crate::host::node_host::ChildHandlePhase::Open)
                    || process.handle_phase == crate::host::node_host::ChildHandlePhase::Closing
                    || process.stdin.has_pending()
                    || process.ipc.as_ref().is_some_and(
                        super::child_process_ipc_shared_vm::IpcChannel::parent_has_work,
                    )
                    || process.stdout_pipe.is_referenced()
                    || process.stderr_pipe.is_referenced()
            })
    }
    #[cfg(not(unix))]
    {
        shared_state
            .borrow()
            .child_processes
            .values()
            .any(|process| {
                process.referenced && process.exit.is_none()
                    || !process.stdout_terminal
                    || !process.stderr_terminal
                    || process.stdin.has_pending()
            })
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
    let command = super::child_process_options_shared_vm::file_text(context, command_arg)?;
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

    let timeout = sync_timeout(context, options)?;
    let input = sync_input(context, options)?;
    #[cfg(unix)]
    let kill_signal = sync_kill_signal(context, options)?;
    let stdio = super::child_process_stdio_shared_vm::normalize_sync(context, options)?;
    let mut process = Command::new(&command);
    let argv0 = options
        .map(|root| property_string(context, root, "argv0"))
        .transpose()?
        .flatten();
    set_command_arg0(&mut process, argv0.as_deref());
    process.args(&arguments);
    process.stdin(
        stdio[0]
            .os()
            .map_err(|error| RootedError::host(error.to_string()))?,
    );
    process.stdout(
        stdio[1]
            .os()
            .map_err(|error| RootedError::host(error.to_string()))?,
    );
    process.stderr(
        stdio[2]
            .os()
            .map_err(|error| RootedError::host(error.to_string()))?,
    );

    let env = options
        .map(|root| property(context, root, "env"))
        .transpose()?;
    if let Some(env) = env.filter(|root| context.is_object_rooted(*root).unwrap_or(false)) {
        super::child_process_env_shared_vm::configure(context, &mut process, env)?;
    }

    let identity =
        super::child_process_options_shared_vm::Identity::from_options(context, options)?;
    let output = match child_cwd(context, &mut process, cwd.as_deref()).and_then(|_| {
        identity.configure(&mut process);
        process.spawn()
    }) {
        Ok(child) => {
            let pid = child.id();
            #[cfg(unix)]
            let collected =
                super::child_process_sync::collect(child, max_buffer, timeout, kill_signal, &input)
                    .map(|result| (result.output, result.error));
            #[cfg(not(unix))]
            let collected = {
                let _ = (max_buffer, timeout, input);
                child.wait_with_output().map(|output| (output, None::<i32>))
            };
            match collected {
                Ok((output, error)) => {
                    let code = error.map(|errno| {
                        super::util_system_error_shared_vm::system_error_name(-f64::from(errno))
                    });
                    make_result(
                        context,
                        Some(pid),
                        output.status.code(),
                        signal_name(&output.status),
                        stdio[1].piped().then_some(output.stdout.as_slice()),
                        stdio[2].piped().then_some(output.stderr.as_slice()),
                        code.as_deref()
                            .map(|code| (error, code, format!("spawnSync {command} {code}"))),
                    )?
                }
                Err(error) => {
                    let code = spawn_error_code(error.raw_os_error());
                    make_result(
                        context,
                        Some(pid),
                        None,
                        None,
                        stdio[1].piped().then_some(&[]),
                        stdio[2].piped().then_some(&[]),
                        Some((
                            error.raw_os_error(),
                            code.as_str(),
                            format!("spawnSync {command} {code}"),
                        )),
                    )?
                }
            }
        }
        Err(error) => {
            let code = spawn_error_code(error.raw_os_error());
            make_result(
                context,
                None,
                None,
                None,
                None,
                None,
                Some((
                    error.raw_os_error(),
                    code.as_str(),
                    format!("spawnSync {command} {code}"),
                )),
            )?
        }
    };
    #[cfg(unix)]
    {
        let error = property(context, output, "error")?;
        if !context
            .rooted_value(error)
            .is_some_and(|value| value.is_null() || value.is_undefined())
        {
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
    project_sync_output(context, output, options)?;
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
    stdout: Option<&[u8]>,
    stderr: Option<&[u8]>,
    error: Option<(Option<i32>, &str, String)>,
) -> Result<RootId, RootedError> {
    let result = context.object_rooted()?;
    if let Some((errno, code, message)) = error {
        let value = context.error_rooted(&message)?;
        if let Some(errno) = errno {
            let errno = context.number(-f64::from(errno));
            set(context, value, "errno", errno)?;
        }
        set_string(context, value, "code", code)?;
        set(context, result, "error", value)?;
    }
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
    let (stdout, stderr, output) = if pid.is_some() {
        let stdout = match stdout {
            Some(bytes) => sync_buffer(context, bytes)?,
            None => context.null(),
        };
        let stderr = match stderr {
            Some(bytes) => sync_buffer(context, bytes)?,
            None => context.null(),
        };
        let null = context.null();
        let output = context.array_rooted(&[null, stdout, stderr])?;
        (stdout, stderr, output)
    } else {
        let undefined = context.undefined();
        let null = context.null();
        (undefined, undefined, null)
    };
    set(context, result, "output", output)?;
    let pid = context.number(pid.unwrap_or(0) as f64);
    set(context, result, "pid", pid)?;
    set(context, result, "stdout", stdout)?;
    set(context, result, "stderr", stderr)?;
    Ok(result)
}

fn sync_buffer(
    context: &mut NativeContext<'_, NodeHost>,
    bytes: &[u8],
) -> Result<RootId, RootedError> {
    let prototype = context.host_function_data()?;
    let buffer = context.uint8_array_rooted(bytes)?;
    context.set_prototype_rooted(buffer, prototype)?;
    Ok(buffer)
}

fn project_sync_output(
    context: &mut NativeContext<'_, NodeHost>,
    result: RootId,
    options: Option<RootId>,
) -> Result<(), RootedError> {
    let Some(options) = options else {
        return Ok(());
    };
    let encoding = property(context, options, "encoding")?;
    if !context.truthy_rooted(encoding)?
        || context.string_text(encoding)?.as_deref() == Some("buffer")
    {
        return Ok(());
    }
    let output = property(context, result, "output")?;
    if context
        .rooted_value(output)
        .is_some_and(|value| value.is_null())
    {
        return Ok(());
    }
    for (index, name) in [(1, "stdout"), (2, "stderr")] {
        let buffer = property(context, result, name)?;
        if context
            .rooted_value(buffer)
            .is_some_and(|value| value.is_null())
        {
            continue;
        }
        let to_string = property(context, buffer, "toString")?;
        let text = context.call_rooted(to_string, buffer, &[encoding])?;
        set(context, result, name, text)?;
        set(context, output, &index.to_string(), text)?;
    }
    Ok(())
}

pub(crate) fn string_array(
    context: &mut NativeContext<'_, NodeHost>,
    array: RootId,
) -> Result<Vec<String>, RootedError> {
    let length = property(context, array, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default()
        .max(0.0) as usize;
    (0..length)
        .map(|index| {
            let key = context.string_rooted(&index.to_string());
            let value = context.get_property_rooted(array, key)?;
            super::child_process_options_shared_vm::null_check(
                context,
                value,
                &format!("args[{index}]"),
            )?;
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
    let value = validate_cwd(context, value)?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(None);
    }
    Ok(Some(context.to_string(value)?))
}

pub(crate) fn validate_cwd(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|v| v.is_undefined() || v.is_null())
    {
        return Ok(value);
    }
    if let Some(path) = context.string_text(value)? {
        reject_cwd_null(context, value, path.as_bytes().contains(&0))?;
        return Ok(value);
    }
    if super::url_shared_vm::url_argument_text(context, value)?.is_some() {
        let undefined = context.undefined();
        let converted = super::url_shared_vm::file_url_to_path(context, undefined, &[value])?;
        let path = context
            .string_text(converted)?
            .ok_or_else(|| RootedError::host("file URL cwd is not a string"))?;
        reject_cwd_null(context, converted, path.as_bytes().contains(&0))?;
        return Ok(converted);
    }
    if context.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::Uint8Array)? {
        let bytes = context
            .view_bytes_rooted(value)
            .ok_or_else(|| RootedError::host("cwd byte view unavailable"))?;
        reject_cwd_null(context, value, bytes.contains(&0))?;
        return Ok(value);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        value,
        "options.cwd",
        "of type string or an instance of Buffer or URL",
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
    let error=context.type_error_rooted(&format!("The property 'options.cwd' must be a string, Uint8Array, or URL without null bytes. Received {detail}"))?;
    let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
    set(context, error, "code", code)?;
    Err(context.throw(error))
}

fn property_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<String>, RootedError> {
    let value = property(context, object, name)?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined() || value.is_null())
    {
        Ok(None)
    } else {
        context.string_text(value)
    }
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
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
            "cannot set child_process result {name}"
        )))
    }
}

#[cfg(unix)]
/// Sends a waiter's observed exit `status` (`None`: the wait failed) to the loop.
fn send_exit(
    sender: &mpsc::Sender<crate::host::node_host::ChildProcessEvent>,
    status: Option<std::process::ExitStatus>,
) {
    let (code, signal) = status
        .map(|status| (status.code(), signal_name(&status).map(str::to_owned)))
        .unwrap_or((None, None));
    let _ = sender.send(crate::host::node_host::ChildProcessEvent::Exit(code, signal));
}

/// The bit position of an exit code in a wait status (`WEXITSTATUS`); a signal's wait status
/// is its number (`WTERMSIG`), with `WAIT_CORE_DUMPED` for a dump.
#[cfg(target_os = "linux")]
const WAIT_EXIT_CODE_SHIFT: u32 = 8;
#[cfg(target_os = "linux")]
const WAIT_CORE_DUMPED: i32 = 0x80;

/// Blocks until child `pid`, which the caller owns unreaped, exits, and returns its status
/// without reaping it (`waitid` with `WNOWAIT`). `None` when the wait fails.
#[cfg(target_os = "linux")]
fn unreaped_exit_status(pid: u32) -> Option<std::process::ExitStatus> {
    use std::os::unix::process::ExitStatusExt;
    loop {
        // SAFETY: `waitid` only writes the zeroed `siginfo_t` it is given.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT)
        };
        if result < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        // SAFETY: a successful `waitid` for an exited child fills the child fields.
        let status = unsafe { info.si_status() };
        let raw = match info.si_code {
            libc::CLD_EXITED => status << WAIT_EXIT_CODE_SHIFT,
            libc::CLD_DUMPED => status | WAIT_CORE_DUMPED,
            _ => status,
        };
        return Some(std::process::ExitStatus::from_raw(raw));
    }
}

#[cfg(not(target_os = "linux"))]
fn unreaped_exit_status(_: u32) -> Option<std::process::ExitStatus> {
    None
}

fn signal_name(status: &std::process::ExitStatus) -> Option<&'static str> {
    use std::os::unix::process::ExitStatusExt;
    let signal = status.signal()?;
    #[cfg(target_os = "linux")]
    {
        super::process_signals_state::SIGNALS
            .iter()
            .find_map(|(name, number)| (*number == signal).then_some(*name))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Some(match signal {
            9 => "SIGKILL",
            15 => "SIGTERM",
            2 => "SIGINT",
            _ => "SIGTERM",
        })
    }
}

#[cfg(not(unix))]
fn signal_name(_: &std::process::ExitStatus) -> Option<&'static str> {
    None
}

/// normalizeSpawnArguments emits DEP0190 once across sync and async calls.
pub(crate) fn shell_args_warning(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let arguments = args.first().copied().unwrap_or(undefined);
    let key = context.string_rooted("length");
    let length = context.get_property_rooted(arguments, key)?;
    if !context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .is_some_and(|length| length > 0.0)
    {
        return Ok(undefined);
    }
    let shared = context.host_mut().shared_state();
    if shared.borrow().child_process_shell_args_warning_emitted {
        return Ok(undefined);
    }
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

fn sync_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<Option<Duration>, RootedError> {
    let Some(options) = options else {
        return Ok(None);
    };
    let value = property(context, options, "timeout")?;
    super::child_process_exec_shared_vm::validate_limit(context, value, "timeout", true)?;
    Ok(context
        .rooted_value(value)
        .and_then(|value| value.as_number())
        .filter(|value| *value > 0.0)
        .map(|value| Duration::from_millis(value as u64)))
}
fn sync_input(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let Some(options) = options else {
        return Ok(Vec::new());
    };
    let value = property(context, options, "input")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(Vec::new());
    }
    if context.string_code_units(value)?.is_some() {
        let encoding = property(context, options, "encoding")?;
        let encoding = context.truthy_rooted(encoding)?.then_some(encoding);
        return super::buffer_shared_vm::encode_bytes(context, value, encoding);
    }
    if let Some(bytes) = context.view_bytes_rooted(value) {
        return Ok(bytes);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        value,
        "options.stdio[0]",
        "of type string or an instance of Buffer, TypedArray, or DataView",
    )?)
}
#[cfg(unix)]
pub(crate) fn sync_kill_signal(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<i32, RootedError> {
    let Some(options) = options else {
        return Ok(libc::SIGTERM);
    };
    let value = property(context, options, "killSignal")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(libc::SIGTERM);
    }
    let name = context.string_text(value)?;
    let number = context
        .rooted_value(value)
        .and_then(|value| value.as_number());
    if name.is_none() && number.is_none() {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            value,
            "options.killSignal",
            "one of type string or number",
        )?);
    }
    #[cfg(target_os = "linux")]
    {
        let name = name.map(|name| name.to_uppercase());
        if let Some(signal) =
            super::process_signals_state::SIGNALS
                .iter()
                .find_map(|(candidate, signal)| {
                    (name.as_deref() == Some(*candidate) || number == Some(f64::from(*signal)))
                        .then_some(*signal)
                })
        {
            return Ok(signal);
        }
        let value = context.to_string(value)?;
        let error = context.type_error_rooted(&format!("Unknown signal: {value}"))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_UNKNOWN_SIGNAL");
        context.define_data_property_rooted(error, key, code, true, true, true)?;
        Err(context.throw(error))
    }
    #[cfg(not(target_os = "linux"))]
    Ok(libc::SIGTERM)
}

/// Node's generic disposable observes public killed/kill hooks and returns
/// undefined, retaining the original thrown value from either hook.
pub(crate) fn dispose_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let child = args.first().copied().unwrap_or(undefined);
    let killed = property(context, child, "killed")?;
    if !context.truthy_rooted(killed)? {
        let kill = property(context, child, "kill")?;
        context.call_rooted(kill, child, &[])?;
    }
    Ok(undefined)
}

fn install_process_handle(
    context: &mut NativeContext<'_, NodeHost>,
    id: u64,
    child: RootId,
) -> Result<(), RootedError> {
    let handle = context.object_rooted()?;
    let id_root = context.number(id as f64);
    for (name, reference) in [("ref", true), ("unref", false)] {
        let reference = context.boolean(reference);
        let data = context.array_rooted(&[id_root, handle, reference])?;
        let method = context.host_function_with_data(
            crate::host::shared_vm::operation("childProcessHandleReference"),
            data,
        )?;
        let key = context.string_rooted("name");
        let name_root = context.string_rooted(name);
        context.define_data_property_rooted(method, key, name_root, false, false, true)?;
        set(context, handle, name, method)?;
    }
    let data = context.array_rooted(&[id_root, handle])?;
    let has_ref = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessHandleHasRef"),
        data,
    )?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("hasRef");
    context.define_data_property_rooted(has_ref, key, name, false, false, true)?;
    set(context, handle, "hasRef", has_ref)?;
    let pid = property(context, child, "pid")?;
    set(context, handle, "pid", pid)?;
    set(context, child, "_handle", handle)
}

pub(crate) fn reference_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let child = args.first().copied().unwrap_or(undefined);
    let handle = property(context, child, "_handle")?;
    if context.truthy_rooted(handle)? {
        // Node's guard and public method call perform two observable gets.
        let handle = property(context, child, "_handle")?;
        let reference = args.get(1).copied().unwrap_or(undefined);
        let name = if context.truthy_rooted(reference)? {
            "ref"
        } else {
            "unref"
        };
        let method = property(context, handle, name)?;
        context.call_rooted(method, handle, &[])?;
    }
    Ok(undefined)
}

fn handle_owner(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
) -> Result<(u64, RootId), RootedError> {
    let data = context.host_function_data()?;
    let handle = property(context, data, "1")?;
    if !context.strict_equal_rooted(receiver, handle)? {
        let error = context.type_error_rooted("Illegal invocation")?;
        return Err(context.throw(error));
    }
    let id = property(context, data, "0")?;
    let id = context
        .rooted_value(id)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("child handle owner missing"))? as u64;
    Ok((id, data))
}

pub(crate) fn reference_handle(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, data) = handle_owner(context, receiver)?;
    let reference = property(context, data, "2")?;
    let reference = context.truthy_rooted(reference)?;
    if let Some(process) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .child_processes
        .get_mut(&id)
    {
        if process.handle_phase != crate::host::node_host::ChildHandlePhase::Closed {
            process.referenced = reference;
        }
    }
    Ok(context.undefined())
}
pub(crate) fn handle_has_ref(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, _) = handle_owner(context, receiver)?;
    let reference = context
        .host_mut()
        .shared_state()
        .borrow()
        .child_processes
        .get(&id)
        .filter(|process| process.handle_phase != crate::host::node_host::ChildHandlePhase::Closed)
        .map(|process| process.referenced);
    Ok(match reference {
        Some(reference) => context.boolean(reference),
        None => context.undefined(),
    })
}

pub(crate) fn prototype_spawn(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let options = args.first().copied().unwrap_or(undefined);
    if !context.is_object_rooted(options)? || context.is_array_rooted(options)? || context.is_callable_rooted(options)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, options, "options", "object",
        )?);
    }
    let backend = context.object_rooted()?;
    let stdio_value = property(context, options, "stdio")?;
    set(context, backend, "stdio", stdio_value)?;
    let stdio = super::child_process_stdio_shared_vm::normalize(context, Some(backend))?;
    let serialization = property(context, options, "serialization")?;
    super::child_process_options_shared_vm::validate_serialization(context, serialization)?;
    set(context, backend, "serialization", serialization)?;
    let file = property(context, options, "file")?;
    if context.string_text(file)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(
            context, file, "options.file", "string",
        )?);
    }
    set(context, receiver, "spawnfile", file)?;
    let argv = property(context, options, "args")?;
    let spawnargs = if context.rooted_value(argv).is_some_and(|v| v.is_undefined()) {
        context.array_rooted(&[])?
    } else {
        if !context.is_array_rooted(argv)? {
            return Err(super::util_shared_vm::argument_type_error_with_expected(
                context, argv, "options.args", "an instance of Array",
            )?);
        }
        argv
    };
    set(context, receiver, "spawnargs", spawnargs)?;
    let values = string_array(context, spawnargs)?;
    // Command's executable is options.file; SDK options.args includes argv[0].
    // Preserve the original argv array identity on the child.
    let tail = values.iter().skip(1).map(|s| context.string_rooted(s)).collect::<Vec<_>>();
    let tail = context.array_rooted(&tail)?;
    // Snapshot normalized data so backend adaptation cannot mutate SDK options.
    for name in ["cwd", "uid", "gid", "detached",
                 "windowsHide", "windowsVerbatimArguments", "signal", "timeout", "killSignal", "env"] {
        let value = property(context, options, name)?;
        set(context, backend, name, value)?;
    }
    if let Some(argv0) = values.first() {
        let argv0 = context.string_rooted(argv0);
        set(context, backend, "argv0", argv0)?;
    }
    let pairs = property(context, options, "envPairs")?;
    if !context.rooted_value(pairs).is_some_and(|v| v.is_undefined()) {
        let env = context.object_rooted()?;
        // Null prototype prevents __proto__ keys changing environment semantics.
        let null = context.null();
        context.set_prototype_rooted(env, null)?;
        for pair in string_array(context, pairs)? {
            // Windows '=C:=...' entries require splitting after the initial '='.
            let offset = usize::from(pair.starts_with('='));
            if let Some(index) = pair[offset..].find('=').map(|i| i + offset) {
                let key = context.string_rooted(&pair[..index]);
                let value = context.string_rooted(&pair[index + 1..]);
                context.define_data_property_rooted(env, key, value, true, true, true)?;
            }
        }
        set(context, backend, "env", env)?;
    }
    let started = spawn_child_with_stdio(context, &[file, tail, backend], Some(stdio))?;
    let errno = property(context, started, "errorNumber")?;
    let failed = context.rooted_value(errno).is_some_and(|v| !v.is_null());
    let lifecycle = context.host_function_data()?;
    // Existing initializer owns real Readable/Writable construction, register_child,
    // output/exit/close dispatch, timeout/abort hooks, and stream-close accounting.
    let completion = (|| {
        context.call_rooted(lifecycle, receiver, &[file, tail, started, backend])?;
        Ok(if failed { errno } else { context.number(0.0) })
    })();
    if completion.is_err() { cleanup_failed_initialization(context, started); }
    completion
}

/// The backend waiter owns the actual Child; cancellation cannot target a reused pid.
/// Drop real pipe/IPC owners and retained receiver roots even after partial register.
fn cleanup_failed_initialization(context: &mut NativeContext<'_, NodeHost>, started: RootId) {
    let id = property(context, started, "id").ok().and_then(|root|
        context.rooted_value(root).and_then(|v| v.as_number())).map(|v| v as u64);
    let process = id.and_then(|id| context.host_mut().shared_state().borrow_mut()
        .child_processes.remove(&id));
    if let Some(process) = process {
        let _ = process.cancel.cancel();
        for root in [process.child, process.stdout, process.stderr, process.stdin_stream]
            .into_iter().flatten() {
            context.release_root(root);
        }
        // Dropping process closes native pipes and IPC transport.
    }
}

/// Public projection constructs first, then observes the mutable prototype hook.
/// This keeps real SDK MockTracker interception on the actual child receiver.
pub(crate) fn construct_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let constructor = property(context, data, "constructor")?;
    let child = context.construct_rooted(constructor, constructor, &[])?;
    let file = args[0];
    let tail = args[1];
    let undefined = context.undefined();
    let original = args.get(2).copied().unwrap_or(undefined);
    let options = context.object_rooted()?;
    for name in ["cwd", "stdio", "uid", "gid", "detached", "serialization", "env",
                 "windowsHide", "windowsVerbatimArguments", "signal", "timeout", "killSignal"] {
        let value = if context.is_object_rooted(original)? {
            property(context, original, name)?
        } else { undefined };
        set(context, options, name, value)?;
    }
    let argv0 = if context.is_object_rooted(original)? {
        property(context, original, "argv0")?
    } else { undefined };
    let argv0 = if context.rooted_value(argv0).is_some_and(|v| v.is_undefined()) {
        file
    } else { argv0 };
    let mut argv = vec![argv0];
    for text in string_array(context, tail)? {
        argv.push(context.string_rooted(&text));
    }
    let argv = context.array_rooted(&argv)?;
    set(context, options, "args", argv)?;
    set(context, options, "file", file)?;
    let spawn = property(context, child, "spawn")?;
    context.call_rooted(spawn, child, &[options])?;
    Ok(child)
}

/// Cancellation owns a kernel process identity on Linux. The owned-Child channel
/// fallback is retained on other platforms and kernels denying pidfd_open.
pub(crate) enum Cancellation {
    #[cfg(target_os = "linux")]
    PidFd(std::os::fd::OwnedFd),
    OwnedChild(mpsc::Sender<()>),
}
impl Cancellation {
    fn new(pid: u32) -> (Self, Option<mpsc::Receiver<()>>) {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::FromRawFd;
            // Called while this thread still owns the unreaped Child, before
            // starting its waiter. PID reuse is therefore impossible here.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
            if fd >= 0 {
                let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) };
                return (Self::PidFd(fd), None);
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = pid;
        let (sender, receiver) = mpsc::channel();
        (Self::OwnedChild(sender), Some(receiver))
    }
    fn cancel(&self) -> std::io::Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Self::PidFd(fd) => {
                use std::os::fd::AsRawFd;
                let result = unsafe {
                    libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(),
                        libc::SIGKILL, std::ptr::null::<libc::siginfo_t>(), 0u32)
                };
                if result < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
            }
            Self::OwnedChild(sender) => sender.send(()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "child waiter has exited")
            }),
        }
    }
}

/// Failure still owns genuine guest pipe streams, but never an OS child.
/// SDK onErrorNT ordering: set exitCode, destroy stdin, emit error, drain outputs;
/// close follows all counted output close events and never emits spawn/exit.
pub(crate) fn spawn_failed(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let child = args[0];
    let started = args[1];
    let tail = args[2];
    let data = context.object_rooted()?;
    set(context, data, "child", child)?;
    set(context, data, "started", started)?;
    set(context, data, "tail", tail)?;
    let mut remaining = 1;
    for name in ["stdout", "stderr"] {
        let stream = property(context, child, name)?;
        set(context, data, name, stream)?;
        if context.is_object_rooted(stream)? {
            remaining += 1;
            let closed = context.host_function_with_data(
                crate::host::shared_vm::operation("childProcessSpawnFailedClosed"), data,
            )?;
            let once = property(context, stream, "once")?;
            let event = context.string_rooted("close");
            context.call_rooted(once, stream, &[event, closed])?;
        }
    }
    let remaining = context.number(remaining as f64);
    set(context, data, "remaining", remaining)?;
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessSpawnFailedRun"), data,
    )?;
    let global = context.global_root()?;
    let process = property(context, global, "process")?;
    let next_tick = property(context, process, "nextTick")?;
    context.call_rooted(next_tick, process, &[callback])?;
    Ok(context.undefined())
}

fn emit_failure_event(
    context: &mut NativeContext<'_, NodeHost>, child: RootId, event: &str,
    arguments: &[RootId],
) -> Result<(), RootedError> {
    let emit = property(context, child, "emit")?;
    let event = context.string_rooted(event);
    let mut args = vec![event];
    args.extend_from_slice(arguments);
    context.call_rooted(emit, child, &args)?;
    Ok(())
}

pub(crate) fn spawn_failed_run(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let child = property(context, data, "child")?;
    let started = property(context, data, "started")?;
    let errno = property(context, started, "errorNumber")?;
    set(context, child, "exitCode", errno)?;
    let null = context.null();
    set(context, child, "_handle", null)?;
    let stdin = property(context, child, "stdin")?;
    if context.is_object_rooted(stdin)? {
        let destroy = property(context, stdin, "destroy")?;
        context.call_rooted(destroy, stdin, &[])?;
    }
    let file = property(context, child, "spawnfile")?;
    let file_text = context.to_string(file)?;
    let syscall = if file_text.is_empty() { "spawn".to_owned() }
                  else { format!("spawn {file_text}") };
    let code = property(context, started, "errorCode")?;
    let code_text = context.to_string(code)?;
    let error = context.error_rooted(&format!("{syscall} {code_text}"))?;
    set(context, error, "code", code)?;
    set(context, error, "errno", errno)?;
    set_string(context, error, "syscall", &syscall)?;
    if !file_text.is_empty() { set(context, error, "path", file)?; }
    let tail = property(context, data, "tail")?;
    set(context, error, "spawnargs", tail)?;
    emit_failure_event(context, child, "error", &[error])?;
    // Native failed-pipe completion belongs after the current tick/microtask
    // drain, like other owned native close completions in the host scheduler.
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessSpawnFailedFlush"), data,
    )?;
    let shared = context.host_mut().shared_state();
    let id = shared.borrow_mut().scheduler.reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("Child close completion IDs exhausted"))?;
    let callback = context.retain(callback)?;
    let undefined = context.undefined();
    let receiver = match context.retain(undefined) {
        Ok(root) => root,
        Err(error) => { context.release_root(callback); return Err(error); }
    };
    shared.borrow_mut().scheduler.queue_shared_immediate(id,
        super::shared_event_loop::SharedImmediateOwner::Host,
        super::shared_event_loop::SharedCallback {
            execution: None, callback, receiver, args: Vec::new(),
        });
    Ok(undefined)
}

pub(crate) fn spawn_failed_flush(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let null = context.null();
    for name in ["stdout", "stderr"] {
        let stream = property(context, data, name)?;
        if context.is_object_rooted(stream)? {
            let push = property(context, stream, "push")?;
            context.call_rooted(push, stream, &[null])?;
            let resume = property(context, stream, "resume")?;
            context.call_rooted(resume, stream, &[])?;
        }
    }
    failed_close_progress(context, data)?;
    Ok(context.undefined())
}

fn failed_close_progress(
    context: &mut NativeContext<'_, NodeHost>, data: RootId,
) -> Result<(), RootedError> {
    let remaining = property(context, data, "remaining")?;
    let remaining = context.rooted_value(remaining).and_then(|v| v.as_number())
        .ok_or_else(|| RootedError::host("spawn failure close count missing"))? - 1.0;
    let value = context.number(remaining);
    set(context, data, "remaining", value)?;
    if remaining == 0.0 {
        let child = property(context, data, "child")?;
        let code = property(context, child, "exitCode")?;
        let signal = property(context, child, "signalCode")?;
        emit_failure_event(context, child, "close", &[code, signal])?;
    }
    Ok(())
}

pub(crate) fn spawn_failed_closed(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    failed_close_progress(context, data)?;
    Ok(context.undefined())
}

pub(crate) fn spawn_succeeded(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let child = args[0];
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("childProcessSpawnSucceededRun"), child,
    )?;
    let global = context.global_root()?;
    let process = property(context, global, "process")?;
    let next_tick = property(context, process, "nextTick")?;
    context.call_rooted(next_tick, process, &[callback])?;
    Ok(context.undefined())
}

pub(crate) fn spawn_succeeded_run(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let child = context.host_function_data()?;
    emit_failure_event(context, child, "spawn", &[])?;
    Ok(context.undefined())
}

pub(crate) fn fork_child_projection(context:&mut NativeContext<'_,NodeHost>,_:RootId,_:&[RootId])->Result<RootId,RootedError>{
    let owner=context.host_function_data()?;
    let policy=crate::host::shared_vm::commonjs::node_library_module(context,"child_process.js","child_process.js")?;
    let function=property(context,policy,"_forkChild")?;
    let key=context.string_rooted("_forkChild");
    context.define_data_property_rooted(owner,key,function,true,true,true)?;
    Ok(context.undefined())
}
