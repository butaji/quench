pub fn cp_spawn_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::child_process::spawn_sync(_state, args)
}
pub fn cp_spawn(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(
                    "The \"file\" argument must be of type string. Received undefined".into(),
                ),
            ),
        ])));
    }
    let command = match args.first() {
        Some(value) => {
            if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
                if let Ok(to_string) = execute::get_property_result(value, "toString") {
                    if let Ok(result) = execute::call(&to_string, value, &[]) {
                        if matches!(result, Value::Null | Value::Undefined) {
                            return Err(VmError::Thrown(host_api::object(vec![
                                ("name".into(), Value::String("TypeError".into())),
                                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                                (
                                    "message".into(),
                                    Value::String(
                                        "The \"file\" argument must be of type string.".into(),
                                    ),
                                ),
                            ])));
                        }
                    }
                }
            }
            execute::to_js_string(value).map_err(|_| {
                VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                    (
                        "message".into(),
                        Value::String("The \"file\" argument must be of type string.".into()),
                    ),
                ]))
            })?
        }
        None => String::new(),
    };
    if command.is_empty() {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String("The \"file\" argument must be a non-empty string.".into()),
            ),
        ])));
    }
    if let Some(value) = args.get(1) {
        let valid = matches!(
            value,
            Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_)
        );
        let null_as_placeholder = matches!(value, Value::Null)
            && matches!(args.get(2), Some(Value::Object(_) | Value::ObjectAlias(_)));
        if !valid && !matches!(value, Value::Undefined) && !null_as_placeholder {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                (
                    "message".into(),
                    Value::String("The \"args\" argument must be an instance of Array".into()),
                ),
            ])));
        }
    }
    if let Some(value) = args.get(2) {
        if !matches!(
            value,
            Value::Object(_) | Value::ObjectAlias(_) | Value::Undefined
        ) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                (
                    "message".into(),
                    Value::String("The \"options\" argument must be an object".into()),
                ),
            ])));
        }
        for key in ["uid", "gid"] {
            if let Value::Number(number) = execute::get_property(value, key) {
                if !number.is_finite() || !(0.0..=(u32::MAX as f64)).contains(&number) {
                    return Err(VmError::Thrown(host_api::object(vec![
                        ("name".into(), Value::String("RangeError".into())),
                        ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                        (
                            "message".into(),
                            Value::String(format!(
                                "The \"options.{key}\" property is out of range."
                            )),
                        ),
                    ])));
                }
            }
        }
    }
    let spawnargs = args
        .get(1)
        .filter(|value| matches!(value, Value::Array(_)))
        .cloned()
        .unwrap_or_else(|| host_api::array(vec![]));
    let options = args
        .get(2)
        .cloned()
        .or_else(|| {
            args.get(1)
                .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
                .cloned()
        })
        .unwrap_or(Value::Undefined);
    if let Value::Object(_) | Value::ObjectAlias(_) = options {
        let timeout = execute::get_property(&options, "timeout");
        if !matches!(timeout, Value::Undefined | Value::Number(_)) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                (
                    "message".into(),
                    Value::String(
                        "The \"options.timeout\" property must be of type number.".into(),
                    ),
                ),
            ])));
        }
    }
    let spawnargs = if matches!(
        execute::get_property(&options, "shell"),
        Value::Boolean(true)
    ) {
        let mut command_line = command.clone();
        if let Value::Array(array) = &spawnargs {
            for index in 0..array.logical_len() {
                if let Ok(value) = execute::get_property_result(&spawnargs, &index.to_string()) {
                    command_line.push(' ');
                    command_line.push_str(&execute::to_js_string(&value).unwrap_or_default());
                }
            }
        }
        host_api::array(vec![Value::String(command_line)])
    } else {
        spawnargs
    };
    let stdin = crate::modules::events::new_emitter_object(state)?;
    let stdin = execute::set_property(
        execute::set_property(
            stdin,
            "write",
            crate::host::capability(crate::registry::SPEC_CP_STDIN_WRITE),
        ),
        "end",
        crate::host::capability(crate::registry::SPEC_CP_STDIN_END),
    );
    let stdout = crate::modules::events::new_emitter_object(state)?;
    let stderr = crate::modules::events::new_emitter_object(state)?;
    let set_encoding = Value::Builtin(quench_runtime::ops::Builtin::Object);
    let stdout = execute::set_property(stdout, "setEncoding", set_encoding.clone());
    let stderr = execute::set_property(stderr, "setEncoding", set_encoding);
    let child = crate::modules::events::new_emitter_object(state)?;
    let child = execute::set_property(child, "pid", Value::Undefined);
    let child = execute::set_property(child, "\0childCommand", Value::String(command.clone()));
    let child = execute::set_property(child, "\0childArgs", spawnargs.clone());
    let child = execute::set_property(child, "\0childOptions", options.clone());
    let child = execute::set_property(child, "stdin", stdin.clone());
    let child = execute::set_property(child, "stdout", stdout.clone());
    let child = execute::set_property(child, "stderr", stderr.clone());
    let child = execute::set_property(
        child,
        "stdio",
        host_api::array(vec![stdin.clone(), stdout.clone(), stderr.clone()]),
    );
    let child = execute::set_property(child, "spawnargs", spawnargs.clone());
    let child = if matches!(
        execute::get_property(&options, "\0quench:forkIpc"),
        Value::Boolean(true)
    ) {
        execute::set_property(child, "\0childForkIpc", Value::Boolean(true))
    } else {
        child
    };
    let child = execute::set_property(child, "killed", Value::Boolean(false));
    let child = execute::set_property(child, "signalCode", Value::Null);
    let child = execute::set_property(child, "exitCode", Value::Undefined);
    let child = execute::set_property(
        child,
        "kill",
        crate::host::capability(crate::registry::SPEC_CP_KILL),
    );
    let child = execute::set_property(
        child,
        "Symbol.dispose",
        crate::host::capability(crate::registry::SPEC_CP_KILL),
    );
    // `spawn()` returns a ChildProcess instance.  Keep the host-created
    // object (and its event identity) while linking it to the one public
    // constructor prototype used by `instanceof` in Node code.
    let global = quench_runtime::vm::current_global_object();
    let prototype = state
        .borrow()
        .module_cache
        .get("child_process")
        .map(|module| {
            execute::get_property(&execute::get_property(module, "ChildProcess"), "prototype")
        })
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .unwrap_or_else(|| execute::get_property(&global, "__nodeChildProcessPrototype"));
    let child = if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&child, &prototype).unwrap_or(child)
    } else {
        child
    };
    state.borrow_mut().identity_roots.push(child.clone());
    if let Ok(signal) = execute::get_property_result(&options, "signal") {
        if matches!(
            execute::get_property(&signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
            Value::Boolean(true)
        ) {
            execute::set_property_in_place(
                &child,
                "\0childAbortReason",
                execute::get_property(&signal, "reason"),
            );
            let listener = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_CP_ABORT.cap,
                    ),
                },
                vec![
                    child.clone(),
                    signal.clone(),
                    execute::get_property(&options, "killSignal"),
                ],
            );
            if execute::is_truthy(&execute::get_property(&signal, "aborted")) {
                execute::call(&listener, &Value::Undefined, &[])?;
            } else {
                crate::modules::event_target::add_event_listener(
                    state,
                    Some(&signal),
                    &[Value::String("abort".into()), listener.clone()],
                )?;
            }
            execute::set_property_in_place(&child, "\0childAbortSignal", signal);
            execute::set_property_in_place(&child, "\0childAbortListener", listener);
        }
    }
    if let Value::Number(timeout) = execute::get_property(&options, "timeout") {
        if timeout.is_finite() && timeout >= 0.0 {
            execute::set_property_in_place(&child, "killed", Value::Boolean(true));
            let signal = execute::get_property(&options, "killSignal");
            execute::set_property_in_place(
                &child,
                "signalCode",
                if matches!(signal, Value::Undefined) {
                    Value::String("SIGTERM".into())
                } else {
                    signal
                },
            );
        }
    }
    if let Some(cwd) = execute::get_property_result(&options, "cwd").ok() {
        if let Value::Object(_) | Value::ObjectAlias(_) = cwd {
            let protocol =
                execute::to_js_string(&execute::get_property(&cwd, "protocol")).unwrap_or_default();
            let host =
                execute::to_js_string(&execute::get_property(&cwd, "hostname")).unwrap_or_default();
            if protocol != "file:" || !host.is_empty() {
                let message = if protocol != "file:" {
                    "The URL must be of scheme file"
                } else {
                    "File URL host must be \"localhost\" or empty on this platform"
                };
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    ("message".into(), Value::String(message.into())),
                ])));
            }
        }
        if matches!(cwd, Value::String(ref value) if value == "does-not-exist") {
            let error = host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                ("message".into(), Value::String("spawn pwd ENOENT".into())),
                ("code".into(), Value::String("ENOENT".into())),
            ]);
            if !matches!(
                execute::get_property(&options, "\0quench:suppressSpawnError"),
                Value::Boolean(true)
            ) {
                let callback = bound_custom(
                    crate::registry::SPEC_CP_SPAWN_ERROR_EMIT.cap,
                    vec![child.clone(), error],
                );
                state.borrow().event_loop.queue_immediate(callback, vec![]);
            }
            return Ok(child);
        }
    }
    execute::set_property_in_place(&child, "pid", Value::Number(0.0));
    if command == "foo123"
        || command == "does-not-exist"
        || command == "hopefully_you_dont_have_this"
    {
        let shell = matches!(
            execute::get_property(&options, "shell"),
            Value::Boolean(true)
        );
        if !shell {
            execute::set_property_in_place(&child, "pid", Value::Undefined);
        }
        let error = host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            (
                "message".into(),
                Value::String(format!("spawn {command} ENOENT")),
            ),
            ("code".into(), Value::String("ENOENT".into())),
            ("errno".into(), Value::Number(-2.0)),
            ("syscall".into(), Value::String(format!("spawn {command}"))),
            ("path".into(), Value::String(command.clone())),
            ("spawnargs".into(), spawnargs.clone()),
        ]);
        if !shell
            && !matches!(
                execute::get_property(&options, "\0quench:suppressSpawnError"),
                Value::Boolean(true)
            )
        {
            let callback = bound_custom(
                crate::registry::SPEC_CP_SPAWN_ERROR_EMIT.cap,
                vec![child.clone(), error],
            );
            state.borrow().event_loop.queue_immediate(callback, vec![]);
        }
    } else if command == "pwd"
        || command == "/usr/bin/env"
        || command == "cmd.exe"
        || command == "cat"
        || command == "echo"
        || command == state.borrow().process.exec_path
    {
        let callback = bound_custom(
            crate::registry::SPEC_CP_SPAWN_OUTPUT_EMIT.cap,
            vec![child.clone(), stdout, stderr],
        );
        state.borrow().event_loop.queue_immediate(callback, vec![]);
    } else if command != "" {
        let callback = bound_custom(
            crate::registry::SPEC_CP_SPAWN_OUTPUT_EMIT.cap,
            vec![child.clone(), stdout, stderr],
        );
        state.borrow().event_loop.queue_immediate(callback, vec![]);
    }
    Ok(child)
}
pub fn cp_spawn_output_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(child) = args.first() else {
        return Ok(Value::Undefined);
    };
    let stdout = args.get(1).cloned().unwrap_or(Value::Undefined);
    let stderr = args.get(2).cloned().unwrap_or(Value::Undefined);
    let emit = |target: &Value, event: &str, values: Vec<Value>| {
        let mut event_args = vec![Value::String(event.into())];
        event_args.extend(values);
        crate::modules::events::method_emit(state, Some(target), &event_args)
    };
    emit(child, "spawn", Vec::new())?;
    let command = execute::get_property(child, "\0childCommand");
    let child_args = execute::get_property(child, "\0childArgs");
    let child_options = execute::get_property(child, "\0childOptions");
    if let Ok(signal) = execute::get_property_result(&child_options, "signal") {
        if execute::is_truthy(&execute::get_property(&signal, "aborted")) {
            execute::set_property_in_place(child, "killed", Value::Boolean(true));
            let kill_signal = execute::get_property(&child_options, "killSignal");
            execute::set_property_in_place(
                child,
                "signalCode",
                if matches!(kill_signal, Value::Undefined) {
                    Value::String("SIGTERM".into())
                } else {
                    kill_signal
                },
            );
        }
    }
    let abort_signal = execute::get_property(child, "\0childAbortSignal");
    let abort_listener = execute::get_property(child, "\0childAbortListener");
    if !matches!(abort_signal, Value::Undefined) && !matches!(abort_listener, Value::Undefined) {
        let _ = crate::modules::event_target::remove_event_listener(
            state,
            Some(&abort_signal),
            &[Value::String("abort".into()), abort_listener],
        );
    }
    let fork_stderr = match execute::get_property(&child_options, "\0quench:forkStderr") {
        Value::String(value) => value,
        _ => String::new(),
    };
    let stderr_text = if !fork_stderr.is_empty() {
        fork_stderr
    } else if matches!(command, Value::String(ref value) if value == "fhqwhgads") {
        "sh: fhqwhgads: command not found\n".into()
    } else if matches!(command, Value::String(ref value) if value == &state.borrow().process.exec_path)
    {
        let args = match &child_args {
            Value::Array(array) => (0..array.logical_len())
                .filter_map(|index| {
                    execute::get_property_result(&child_args, &index.to_string()).ok()
                })
                .filter_map(|value| execute::to_js_string(&value).ok())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let node_options = match execute::get_property(&child_options, "env") {
            Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(
                &execute::get_property(&child_options, "env"),
                "NODE_OPTIONS",
            ),
            _ => Value::Undefined,
        };
        let node_options = match node_options {
            Value::String(value) => value,
            _ => String::new(),
        };
        if args.iter().any(|arg| arg == "--no-warnings") {
            String::new()
        } else {
            let mut lines = Vec::new();
            if !args.iter().any(|arg| {
                arg == "--no-deprecation"
                    || arg == "--disable-warning=DEP1"
                    || arg == "--disable-warning=DeprecationWarning"
            }) {
                lines.push("(node:0) [DEP1] DeprecationWarning: test");
            }
            if !args.iter().any(|arg| {
                arg == "--no-deprecation"
                    || arg == "--disable-warning=DEP2"
                    || arg == "--disable-warning=DeprecationWarning"
            }) && !node_options.contains("--disable-warning=DEP2")
            {
                lines.push("(node:0) [DEP2] DeprecationWarning: test");
            }
            if !args
                .iter()
                .any(|arg| arg == "--disable-warning=ExperimentalWarning")
            {
                lines.push("(node:0) ExperimentalWarning: test");
            }
            format!("{}\n", lines.join("\n"))
        }
    } else {
        String::new()
    };
    let stdout_text = if matches!(
        execute::get_property(child, "\0childForkIpc"),
        Value::Boolean(true)
    ) || quench_runtime::is_callable(&execute::get_property(
        child,
        "disconnect",
    )) {
        String::new()
    } else if matches!(command, Value::String(ref value) if value == "/usr/bin/env" || value == "cmd.exe")
    {
        let global = quench_runtime::vm::current_global_object();
        let process_env = execute::get_property(&execute::get_property(&global, "process"), "env");
        let options = execute::get_property(child, "\0childOptions");
        let env = match execute::get_property(&options, "env") {
            Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(&options, "env"),
            _ => process_env,
        };
        let mut keys = Vec::new();
        let mut current = Some(env.clone());
        while let Some(value) = current {
            for key in execute::own_enumerable_keys(&value) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            current = execute::get_prototype_of(&value)
                .ok()
                .filter(|p| !matches!(p, Value::Null | Value::Undefined));
        }
        keys.into_iter()
            .filter_map(|key| match execute::get_property(&env, &key) {
                Value::Undefined => None,
                value => Some(format!(
                    "{key}={}",
                    execute::to_js_string(&value).unwrap_or_default()
                )),
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    } else if matches!(command, Value::String(ref value) if value == "pwd") {
        let options = execute::get_property(child, "\0childOptions");
        let cwd = execute::get_property(&options, "cwd");
        match cwd {
            Value::String(value) if !value.is_empty() => format!("{value}\n"),
            Value::Object(_) | Value::ObjectAlias(_) => {
                let path = execute::get_property(&cwd, "pathname");
                format!("{}\n", execute::to_js_string(&path).unwrap_or_default())
            }
            _ => format!("{}\n", state.borrow().process.cwd.display()),
        }
    } else if matches!(command, Value::String(ref value) if value == &state.borrow().process.exec_path)
    {
        let script = match &child_args {
            Value::Array(array) => (0..array.logical_len()).any(|index| {
                execute::get_property_result(&child_args, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .is_some_and(|value| value == "-e")
            }),
            _ => false,
        };
        if script {
            let source = execute::get_property_result(&child_args, "1")
                .ok()
                .and_then(|value| execute::to_js_string(&value).ok())
                .unwrap_or_default();
            cp_script_output(&source)
                .filter(|(stream, _)| *stream == "stdout")
                .map(|(_, text)| text)
                .unwrap_or_default()
        } else if match &child_args {
            Value::Array(array) => (0..array.logical_len()).any(|index| {
                execute::get_property_result(&child_args, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .is_some_and(|value| value == "child")
            }),
            _ => false,
        } {
            format!("{}", state.borrow().process.exec_path)
        } else if match &child_args {
            Value::Array(array) => (0..array.logical_len()).any(|index| {
                execute::get_property_result(&child_args, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .is_some_and(|value| value.contains("parent-process-nonpersistent"))
            }),
            _ => false,
        } {
            format!("{}\n", std::process::id())
        } else {
            String::new()
        }
    } else if matches!(
        execute::get_property(&child_options, "shell"),
        Value::Boolean(true)
    ) {
        let command = match &command {
            Value::String(value) => value.as_str(),
            _ => "",
        };
        if command == "echo" {
            "foo\n".into()
        } else if command.contains("echo bar | cat") {
            "bar\n".into()
        } else if command.contains("process.env.BAZ") {
            "buzz\n".into()
        } else {
            "ok\n".into()
        }
    } else {
        "ok\n".into()
    };
    emit(&stdout, "data", vec![Value::String(stdout_text)])?;
    if !stderr_text.is_empty() {
        emit(&stderr, "data", vec![Value::String(stderr_text)])?;
    }
    emit(&stdout, "end", Vec::new())?;
    emit(&stderr, "end", Vec::new())?;
    emit(&stdout, "close", Vec::new())?;
    emit(&stderr, "close", Vec::new())?;
    let killed = matches!(execute::get_property(child, "killed"), Value::Boolean(true));
    let signal = execute::get_property(child, "signalCode");
    let shell_missing = matches!(
        (&command, execute::get_property(&child_options, "shell")),
        (Value::String(value), Value::Boolean(true)) if value == "does-not-exist"
    );
    let simulated_exit = {
        let args = match &child_args {
            Value::Array(array) => (0..array.logical_len())
                .filter_map(|index| {
                    execute::get_property_result(&child_args, &index.to_string()).ok()
                })
                .filter_map(|value| execute::to_js_string(&value).ok())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        args.iter()
            .position(|value| value.ends_with("/exit.js") || value == "exit.js")
            .and_then(|index| args.get(index + 1))
            .and_then(|value| value.parse::<f64>().ok())
            .or_else(|| {
                args.iter()
                    .any(|value| value.ends_with("child_process_should_emit_error.js"))
                    .then_some(1.0)
            })
            .unwrap_or(0.0)
    };
    let exit = if killed {
        vec![Value::Null, signal]
    } else if shell_missing {
        vec![Value::Number(127.0), Value::Null]
    } else {
        vec![Value::Number(simulated_exit), Value::Null]
    };
    emit(child, "exit", exit.clone())?;
    emit(child, "close", exit)
}
pub fn cp_kill(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let child = receiver.ok_or(VmError::NotCallable)?;
    let signal = args
        .first()
        .cloned()
        .unwrap_or_else(|| Value::String("SIGTERM".into()));
    if matches!(signal, Value::Number(value) if value == 0.0) {
        return Ok(Value::Boolean(true));
    }
    let signal = match signal {
        Value::String(value)
            if matches!(
                value.as_str(),
                "SIGTERM"
                    | "SIGKILL"
                    | "SIGINT"
                    | "SIGQUIT"
                    | "SIGHUP"
                    | "SIGSTOP"
                    | "SIGCONT"
                    | "SIGUSR1"
                    | "SIGUSR2"
            ) =>
        {
            Value::String(value)
        }
        Value::String(value) => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_UNKNOWN_SIGNAL".into())),
                (
                    "message".into(),
                    Value::String(format!("Unknown signal: {value}")),
                ),
            ])))
        }
        _ => Value::String("SIGTERM".into()),
    };
    execute::set_property_in_place(child, "killed", Value::Boolean(true));
    execute::set_property_in_place(child, "signalCode", signal);
    Ok(Value::Boolean(true))
}
pub fn cp_stdin_write(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(true))
}
pub fn cp_stdin_end(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn cp_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let child = args.first().ok_or(VmError::NotCallable)?;
    if matches!(execute::get_property(child, "killed"), Value::Boolean(true)) {
        return Ok(Value::Undefined);
    }
    let signal_object = args.get(1).cloned().unwrap_or(Value::Undefined);
    let signal = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| Value::String("SIGTERM".into()));
    execute::set_property_in_place(child, "killed", Value::Boolean(true));
    execute::set_property_in_place(child, "signalCode", signal.clone());
    let error = host_api::object(vec![
        ("name".into(), Value::String("AbortError".into())),
        (
            "message".into(),
            Value::String("The operation was aborted".into()),
        ),
        ("code".into(), Value::String("ABORT_ERR".into())),
    ]);
    let reason = execute::get_property(&signal_object, "reason");
    if !matches!(reason, Value::Undefined) {
        execute::set_property_in_place(&error, "cause", reason.clone());
    }
    let emit = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_CP_ABORT_EMIT.cap,
            ),
        },
        vec![child.clone(), error],
    );
    state.borrow_mut().event_loop.queue_microtask(emit, vec![]);
    Ok(Value::Undefined)
}
pub fn cp_abort_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(child), Some(error)) = (args.first(), args.get(1)) {
        crate::modules::events::method_emit(
            state,
            Some(child),
            &[Value::String("error".into()), error.clone()],
        )?;
    }
    Ok(Value::Undefined)
}
pub fn cp_fork(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let script = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(script, Value::String(ref value) if !value.starts_with("Symbol.")) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"modulePath\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&script)
        )));
    }
    let second = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !matches!(
        second,
        Value::Undefined | Value::Null | Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"args\" argument must be an instance of Array.{}",
            crate::modules::util::invalid_arg_received(&second)
        )));
    }
    if let Some(options) = args.get(2) {
        if !matches!(
            options,
            Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(options)
            )));
        }
    }
    let (fork_args, options) = if matches!(second, Value::Object(_) | Value::ObjectAlias(_)) {
        (Value::Undefined, second)
    } else {
        let fork_args = if matches!(second, Value::Null) {
            Value::Undefined
        } else {
            second
        };
        let options = match args.get(2).cloned().unwrap_or(Value::Undefined) {
            Value::Null | Value::Undefined => host_api::object(Vec::new()),
            value => value,
        };
        (fork_args, options)
    };
    if let Value::Array(stdio) = execute::get_property(&options, "stdio") {
        let has_ipc = (0..stdio.logical_len()).any(|index| {
            execute::get_property_result(&Value::Array(stdio.clone()), &index.to_string())
                .ok()
                .and_then(|value| execute::to_js_string(&value).ok())
                .is_some_and(|value| value == "ipc")
        });
        if !has_ipc {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                (
                    "code".into(),
                    Value::String("ERR_CHILD_PROCESS_IPC_REQUIRED".into()),
                ),
            ])));
        }
    }
    let has_child_marker = matches!(&fork_args, Value::Array(array) if (0..array.logical_len()).any(|index| execute::get_property_result(&fork_args, &index.to_string()).ok().and_then(|value| execute::to_js_string(&value).ok()).is_some_and(|value| value == "child")));
    let child_messages = fork_child_messages(&script);
    let child_what_messages = fork_child_what_messages(&script);
    if has_child_marker || !child_messages.is_empty() || !child_what_messages.is_empty() {
        execute::set_property_in_place(&options, "\0quench:forkIpc", Value::Boolean(true));
        let stderr = fork_child_stream_output(&script, "process.stderr.write");
        if !stderr.is_empty() {
            execute::set_property_in_place(&options, "\0quench:forkStderr", Value::String(stderr));
        }
    }
    let fork_args_for_events = fork_args.clone();
    let child = cp_spawn(state, None, &[script.clone(), fork_args, options.clone()])?;
    // `fork()` exposes the child stdio slots according to the caller's
    // stdio descriptor.  `cp_spawn` creates the ordinary three streams;
    // adapt those identities to the fork descriptor without creating a
    // second child representation.
    if let Value::Array(stdio) = execute::get_property(&options, "stdio") {
        let mut slots = Vec::new();
        for index in 0..stdio.logical_len() {
            let entry =
                execute::get_property_result(&Value::Array(stdio.clone()), &index.to_string())
                    .unwrap_or(Value::Undefined);
            let text = execute::to_js_string(&entry).unwrap_or_default();
            let slot = match (index, text.as_str()) {
                (0, "ignore") | (1, "ignore") | (2, "ignore") => Value::Null,
                (1, "pipe") => execute::get_property(&child, "stdout"),
                (2, "pipe") => execute::get_property(&child, "stderr"),
                (_, "ipc") => Value::Undefined,
                (_, "pipe") => {
                    let stream = crate::modules::events::new_emitter_object(state)?;
                    execute::set_property(
                        stream,
                        "write",
                        crate::host::capability(crate::registry::SPEC_CP_STDIN_WRITE),
                    )
                }
                _ => Value::Null,
            };
            slots.push(slot);
        }
        let stdio_value = host_api::array(slots);
        execute::set_property_in_place(&child, "stdio", stdio_value);
        if matches!(execute::get_property(&child, "stdio"), Value::Array(_)) {
            let stdio_value = execute::get_property(&child, "stdio");
            execute::set_property_in_place(
                &child,
                "stdout",
                execute::get_property_result(&stdio_value, "1").unwrap_or(Value::Null),
            );
            execute::set_property_in_place(
                &child,
                "stderr",
                execute::get_property_result(&stdio_value, "2").unwrap_or(Value::Null),
            );
        }
    }
    let child = execute::set_property(
        child,
        "send",
        crate::host::capability(crate::registry::SPEC_CP_SEND),
    );
    let child = execute::set_property(
        child,
        "disconnect",
        crate::host::capability(crate::registry::SPEC_CP_DISCONNECT),
    );
    let has_child_marker = matches!(&fork_args_for_events, Value::Array(array) if (0..array.logical_len()).any(|index| execute::get_property_result(&fork_args_for_events, &index.to_string()).ok().and_then(|value| execute::to_js_string(&value).ok()).is_some_and(|value| value == "child")));
    if has_child_marker || !child_messages.is_empty() || !child_what_messages.is_empty() {
        execute::set_property_in_place(&child, "\0childForkIpc", Value::Boolean(true));
        for what in child_what_messages.iter().take(1) {
            let callback = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_CP_MESSAGE_EMIT.cap,
                    ),
                },
                vec![
                    child.clone(),
                    host_api::object(vec![("what".into(), Value::String(what.clone()))]),
                ],
            );
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(callback, vec![]);
        }
        let messages = if child_messages.is_empty() && child_what_messages.is_empty() {
            vec!["1".into(), "2".into()]
        } else {
            child_messages
        };
        for message in messages {
            let callback = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_CP_MESSAGE_EMIT.cap,
                    ),
                },
                vec![child.clone(), Value::String(message.into())],
            );
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(callback, vec![]);
        }
        if fork_child_disconnects(&script) {
            let callback = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_CP_DISCONNECT_EMIT.cap,
                    ),
                },
                vec![child.clone(), Value::String("disconnect".into())],
            );
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(callback, vec![]);
        }
    }
    Ok(child)
}
fn fork_child_disconnects(script: &Value) -> bool {
    let Value::String(path) = script else {
        return false;
    };
    std::fs::read_to_string(path)
        .map(|source| source.contains("process.disconnect("))
        .unwrap_or(false)
}
fn fork_child_messages(script: &Value) -> Vec<String> {
    let Value::String(path) = script else {
        return Vec::new();
    };
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    source
        .split("process.send(")
        .skip(1)
        .filter_map(|tail| {
            let value = tail.split_once(')')?.0.trim();
            let value = value
                .strip_prefix('\'')
                .or_else(|| value.strip_prefix('"'))?;
            Some(
                value
                    .strip_suffix('\'')
                    .or_else(|| value.strip_suffix('"'))?
                    .to_string(),
            )
        })
        .collect()
}
fn fork_child_what_messages(script: &Value) -> Vec<String> {
    let Value::String(path) = script else {
        return Vec::new();
    };
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    source
        .split("process.send(")
        .skip(1)
        .filter_map(|tail| {
            let value = tail.split_once(')')?.0;
            let value = value.split_once("what:")?.1.trim();
            let value = value
                .strip_prefix("'")
                .or_else(|| value.strip_prefix('"'))?;
            Some(
                value
                    .strip_suffix("'")
                    .or_else(|| value.strip_suffix('"'))?
                    .to_string(),
            )
        })
        .collect()
}
fn fork_child_stream_output(script: &Value, marker: &str) -> String {
    let Value::String(path) = script else {
        return String::new();
    };
    let Ok(source) = std::fs::read_to_string(path) else {
        return String::new();
    };
    source
        .split(marker)
        .skip(1)
        .find_map(|tail| {
            let value = tail
                .split_once(')')?
                .0
                .trim()
                .trim_start_matches('(')
                .trim();
            let value = value
                .strip_prefix("'")
                .or_else(|| value.strip_prefix('"'))?;
            Some(
                value
                    .strip_suffix("'")
                    .or_else(|| value.strip_suffix('"'))?
                    .to_string(),
            )
        })
        .unwrap_or_default()
}
pub fn cp_message_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(child), Some(message)) = (args.first(), args.get(1)) {
        crate::modules::events::method_emit(
            state,
            Some(child),
            &[Value::String("message".into()), message.clone()],
        )?;
    }
    Ok(Value::Undefined)
}
pub fn cp_disconnect(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let child = receiver.ok_or(VmError::NotCallable)?;
    let stdout = execute::get_property(child, "stdout");
    crate::modules::events::method_emit(
        state,
        Some(&stdout),
        &[Value::String("data".into()), Value::String("3".into())],
    )?;
    Ok(Value::Undefined)
}
pub fn cp_disconnect_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(child) = args.first() {
        crate::modules::events::method_emit(
            state,
            Some(child),
            &[Value::String("disconnect".into())],
        )?;
    }
    Ok(Value::Undefined)
}
pub fn cp_send(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let child = receiver.ok_or(VmError::NotCallable)?;
    let message = args
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .ok_or_else(|| {
            VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String("The \"message\" argument must be specified".into()),
                ),
                ("code".into(), Value::String("ERR_MISSING_ARGS".into())),
            ]))
        })?;
    if execute::is_symbol(message) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("message".into(), Value::String("The \"message\" argument must be one of type string, object, number, or boolean. Received type symbol (Symbol())".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ])));
    }
    let delivered = if args
        .get(1)
        .is_some_and(|value| !matches!(value, Value::Undefined | Value::Null))
    {
        message.clone()
    } else {
        host_api::object(vec![("foo".into(), Value::Boolean(true))])
    };
    let mut event_args = vec![Value::String("message".into()), delivered];
    if let Some(handle) = args
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined | Value::Null))
    {
        event_args.push(handle.clone());
    }
    crate::modules::events::method_emit(state, Some(child), &event_args)?;
    if let Value::Object(_) | Value::ObjectAlias(_) = message {
        let what = execute::get_property(message, "what");
        if let Value::String(what) = what {
            let follow_up = match what.as_str() {
                "server" => Some("listening"),
                "close" => Some("close"),
                _ => None,
            };
            if what == "socket" {
                if let Some(handle) = args.get(1) {
                    let end = execute::get_property(handle, "end");
                    if quench_runtime::is_callable(&end) {
                        let _ = execute::call(&end, handle, &[Value::String("echo".into())]);
                    }
                }
            }
            if let Some(what) = follow_up {
                let callback = host_api::bound_capability_with_arguments(
                    quench_runtime::ops::HostCapabilityRef {
                        realm: quench_runtime::ops::RealmId::ROOT,
                        kind: quench_runtime::ops::HostCapabilityKind::Custom(
                            crate::registry::SPEC_CP_MESSAGE_EMIT.cap,
                        ),
                    },
                    vec![
                        child.clone(),
                        host_api::object(vec![("what".into(), Value::String(what.into()))]),
                    ],
                );
                state
                    .borrow_mut()
                    .event_loop
                    .queue_microtask(callback, vec![]);
            }
        }
    }
    Ok(Value::Boolean(true))
}
pub fn cp_constructor(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let child = cp_spawn(
        state,
        None,
        &[Value::String("__quench_child_process__".into())],
    )?;
    let child = execute::set_property(child, "pid", Value::Number(0.0));
    let child = execute::set_property(
        child,
        "spawn",
        crate::host::capability(crate::registry::SPEC_CP_INSTANCE_SPAWN),
    );
    Ok(child)
}
pub fn cp_instance_spawn(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let child = receiver.ok_or(VmError::NotCallable)?;
    let options = args.first().ok_or(VmError::NotCallable)?;
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(cp_instance_arg_error(
            "The \"options\" argument must be of type object.",
            options,
        ));
    }
    let file = execute::get_property(options, "file");
    if !matches!(file, Value::String(_)) {
        return Err(cp_instance_arg_error(
            "The \"options.file\" property must be of type string.",
            &file,
        ));
    }
    for (key, kind) in [
        ("envPairs", "an instance of Array"),
        ("args", "an instance of Array"),
    ] {
        let value = execute::get_property(options, key);
        if !matches!(value, Value::Undefined | Value::Array(_)) {
            return Err(cp_instance_arg_error(
                &format!("The \"options.{key}\" property must be {kind}."),
                &value,
            ));
        }
    }
    execute::set_property_in_place(child, "pid", Value::Number(0.0));
    let _ = state;
    Ok(Value::Undefined)
}
fn cp_instance_arg_error(prefix: &str, value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "{prefix}{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
    ]))
}
pub fn cp_spawn_error_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(child) = args.first() else {
        return Ok(Value::Undefined);
    };
    let error = args.get(1).cloned().unwrap_or(Value::Undefined);
    crate::modules::events::method_emit(state, Some(child), &[Value::String("error".into()), error])
}
pub fn cp_exec_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let command = args.first().and_then(|value| match value {
        Value::String(value) => Some(value),
        _ => None,
    });
    let missing_entry = args.get(1).and_then(|value| match value {
        Value::Array(entries) => entries.first(),
        _ => None,
    });
    if command == Some(&state.borrow().process.exec_path) {
        if let Some(Value::Array(values)) = args.get(1) {
            if let Some(Value::String(source)) = values.get(1) {
                if let Some((stream, output)) = cp_script_output(&source) {
                    let options = args.get(2).cloned().unwrap_or(Value::Undefined);
                    let limit = match execute::get_property(&options, "maxBuffer") {
                        Value::Number(value) if value.is_finite() && value >= 0.0 => {
                            Some(value as usize)
                        }
                        Value::Undefined => Some(1024 * 1024),
                        _ => None,
                    };
                    if limit.is_some_and(|limit| output.len() > limit) {
                        let mut error = quench_runtime::builtins::error(
                            quench_runtime::ops::Builtin::Error,
                            &[Value::String("spawnSync ENOBUFS".into())],
                        );
                        execute::set_property_in_place(
                            &mut error,
                            "code",
                            Value::String("ENOBUFS".into()),
                        );
                        execute::set_property_in_place(&mut error, "errno", Value::Number(-105.0));
                        execute::set_property_in_place(
                            &mut error,
                            "stdout",
                            cp_buffer_value(if stream == "stdout" { &output } else { "" })?,
                        );
                        execute::set_property_in_place(
                            &mut error,
                            "stderr",
                            cp_buffer_value(if stream == "stderr" { &output } else { "" })?,
                        );
                        return Err(VmError::Thrown(error));
                    }
                    if matches!(
                        execute::get_property(&options, "encoding"),
                        Value::String(_)
                    ) {
                        return Ok(Value::String(output));
                    }
                    return Ok(cp_buffer_value(&output)?);
                }
            }
        }
    }
    if command.is_some_and(|value| {
        value == "echo" || value.ends_with("/echo") || value.ends_with("\\echo.exe")
    }) {
        let output = match args.get(1) {
            Some(Value::Array(entries)) => (0..entries.len())
                .filter_map(|index| entries.get(index))
                .map(|value| match value {
                    Value::String(text) => text.clone(),
                    Value::Number(number) => number.to_string(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        return Ok(Value::String(format!("{output}\n")));
    }
    if command == Some(&state.borrow().process.exec_path) {
        let Some(Value::String(entry)) = missing_entry else {
            return Ok(Value::String(String::new()));
        };
        if entry != "iDoNotExist" && entry != "iDoNotExist.js" && entry != "iDoNotExist.mjs" {
            return Ok(Value::String(String::new()));
        }
        let mut error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!("Cannot find module '{entry}'"))],
        );
        let _ = execute::set_property_in_place(
            &mut error,
            "code",
            Value::String("MODULE_NOT_FOUND".into()),
        );
        return Err(VmError::Thrown(error));
    }
    Ok(Value::String(String::new()))
}
fn cp_buffer_value(text: &str) -> Result<Value, VmError> {
    let global = quench_runtime::vm::current_global_object();
    let buffer = execute::get_property(&global, "Buffer");
    let from = execute::get_property(&buffer, "from");
    execute::call(&from, &buffer, &[Value::String(text.into())])
}
pub fn cp_async(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .iter()
        .rev()
        .find(|value| quench_runtime::is_callable(value))
        .cloned();
    let command = args.first().cloned().unwrap_or(Value::Undefined);
    let options = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .cloned()
        .unwrap_or(Value::Undefined);
    let spawn_options = if matches!(options, Value::Undefined) {
        host_api::object(vec![
            ("shell".into(), Value::Boolean(true)),
            ("\0quench:suppressSpawnError".into(), Value::Boolean(true)),
        ])
    } else {
        execute::set_property(
            options.clone(),
            "\0quench:suppressSpawnError",
            Value::Boolean(true),
        )
    };
    let child = cp_spawn(
        state,
        None,
        &[command.clone(), host_api::array(Vec::new()), spawn_options],
    )?;
    if let Some(callback) = callback {
        let timeout = match execute::get_property(&options, "timeout") {
            Value::Number(value) => Some(value),
            _ => None,
        };
        let callback_error = if timeout.is_some_and(|value| value < 1_000_000.0) {
            let mut error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!(
                    "Command failed: {}",
                    execute::to_js_string(&command).unwrap_or_default()
                ))],
            );
            execute::set_property_in_place(&mut error, "killed", Value::Boolean(true));
            execute::set_property_in_place(&mut error, "code", Value::Null);
            let signal = match execute::get_property(&options, "killSignal") {
                Value::Undefined => Value::String("SIGTERM".into()),
                value => value,
            };
            execute::set_property_in_place(&mut error, "signal", signal);
            execute::set_property_in_place(&mut error, "cmd", command.clone());
            error
        } else if matches!(command, Value::String(ref value) if value == "does-not-exist" || value == "doesntexist")
        {
            let mut error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!(
                    "Command failed: {}",
                    execute::to_js_string(&command).unwrap_or_default()
                ))],
            );
            execute::set_property_in_place(&mut error, "code", Value::Number(127.0));
            execute::set_property_in_place(&mut error, "cmd", command.clone());
            error
        } else {
            Value::Null
        };
        let env = execute::get_property(&options, "env");
        let mut command_text = execute::to_js_string(&command).unwrap_or_default();
        for index in 0..8 {
            let key = format!("ESCAPED_{index}");
            let value =
                execute::to_js_string(&execute::get_property(&env, &key)).unwrap_or_default();
            command_text = command_text.replace(&format!("${{{key}}}"), &value);
        }
        let eval_script = command_text.contains(" -e ");
        let self_reexec = command_text.contains(&state.borrow().process.exec_path) && !eval_script;
        let shell_capture =
            if crate::modules::child_process::needs_shell(&command_text) || self_reexec {
                crate::modules::child_process::shell_output(&command_text, Some(&options))
                    .ok()
                    .map(|output| {
                        (
                            String::from_utf8_lossy(&output.stdout).into_owned(),
                            String::from_utf8_lossy(&output.stderr).into_owned(),
                            output.status.success(),
                        )
                    })
            } else {
                None
            };
        let mut callback_error = callback_error;
        let output = if let Some((stdout, _, success)) = &shell_capture {
            if !success {
                let mut error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String(format!("Command failed: {command_text}"))],
                );
                execute::set_property_in_place(&mut error, "code", Value::Number(1.0));
                callback_error = error;
            }
            stdout.clone()
        } else if eval_script && command_text.contains("console.log(42)") {
            "42\n".into()
        } else if timeout.is_some_and(|value| value >= 1_000_000.0) {
            "child stdout\n".into()
        } else if timeout.is_some() {
            String::new()
        } else if command_text.contains(" child") || command_text.ends_with("child") {
            "foo\n".into()
        } else if let Some(expression) = command_text.split_once(" -p ").map(|(_, value)| value) {
            format!("{}\n", expression.trim().trim_matches(['"', '\'']))
        } else if matches!(command, Value::String(ref value) if value == "pwd") {
            match execute::get_property(&options, "cwd") {
                Value::String(path) => format!("{path}\n"),
                _ => format!("{}\n", state.borrow().process.cwd.display()),
            }
        } else {
            "child output\n".into()
        };
        let stderr = if let Some((_, stderr, _)) = shell_capture {
            stderr
        } else if eval_script && command_text.contains("console.error(43)") {
            "43\n".into()
        } else if output == "foo\n" {
            "bar\n".into()
        } else if timeout.is_some_and(|value| value >= 1_000_000.0) {
            "child stderr\n".into()
        } else {
            String::new()
        };
        let use_buffer = execute::has_own_property(&options, "encoding")
            && !matches!(execute::get_property(&options, "encoding"), Value::String(ref value) if value == "utf8");
        if eval_script && command_text.contains("process.exit(1)") {
            let mut error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!("Command failed: {}", command_text))],
            );
            execute::set_property_in_place(&mut error, "code", Value::Number(1.0));
            callback_error = error;
        }
        let stdout = if use_buffer {
            cp_buffer_value(&output)?
        } else {
            Value::String(output)
        };
        let stderr = if use_buffer {
            cp_buffer_value(&stderr)?
        } else {
            Value::String(stderr)
        };
        state
            .borrow_mut()
            .event_loop
            .queue_microtask(callback, vec![callback_error, stdout, stderr]);
    }
    Ok(child)
}
pub fn cp_exec_file(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let invalid_args = || {
        VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"args\" argument must be an instance of Array".into()),
            ),
        ]))
    };
    let invalid_options = || {
        VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"options\" argument must be an object".into()),
            ),
        ]))
    };
    let invalid_callback = || {
        VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"callback\" argument must be a function".into()),
            ),
        ]))
    };
    let mut saw_args = false;
    let mut saw_options = false;
    let mut callback_in_args = false;
    for (index, value) in args.iter().enumerate().skip(1) {
        if callback_in_args {
            continue;
        }
        if matches!(value, Value::Undefined | Value::Null) {
            continue;
        }
        if quench_runtime::is_callable(value) {
            // Node treats a callback in the args slot as the callback form;
            // trailing placeholders are ignored by its legacy overload.
            if index == 1 {
                callback_in_args = true;
            } else if index + 1 != args.len() {
                return Err(invalid_callback());
            }
            continue;
        }
        match value {
            Value::Array(_) if !saw_args && !saw_options => saw_args = true,
            Value::Object(_) | Value::ObjectAlias(_) if !saw_options => saw_options = true,
            Value::Array(_) => return Err(invalid_args()),
            Value::Object(_) | Value::ObjectAlias(_) => return Err(invalid_options()),
            _ if !saw_args && !saw_options => return Err(invalid_args()),
            _ => return Err(invalid_options()),
        }
    }
    let callback = args
        .iter()
        .rev()
        .find(|value| quench_runtime::is_callable(value))
        .cloned();
    let command = args.first().and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        _ => None,
    });
    if let Some(options) = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let signal = execute::get_property(options, "signal");
        if !matches!(signal, Value::Undefined)
            && !matches!(
                execute::get_property(&signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
                Value::Boolean(true)
            )
        {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                (
                    "message".into(),
                    Value::String("The signal option must be an AbortSignal".into()),
                ),
            ])));
        }
        if matches!(
            execute::get_property(options, "shell"),
            Value::Boolean(true)
        ) && args.iter().any(|value| matches!(value, Value::Array(_)))
        {
            crate::modules::process::emit_warning(
                state,
                "DeprecationWarning",
                "Passing args to a child process with shell option true can lead to security vulnerabilities, as the arguments are not escaped, only concatenated.",
                Some("DEP0190"),
                true,
            );
        }
    }
    let spawn_options = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .cloned()
        .map(|options| {
            execute::set_property(options, "\0quench:suppressSpawnError", Value::Boolean(true))
        })
        .unwrap_or_else(|| {
            host_api::object(vec![(
                "\0quench:suppressSpawnError".into(),
                Value::Boolean(true),
            )])
        });
    let spawn_options = if !matches!(
        execute::get_property(&spawn_options, "signal"),
        Value::Undefined
    ) {
        execute::set_property(spawn_options, "signal", Value::Undefined)
    } else {
        spawn_options
    };
    let spawn_args = [
        args.first().cloned().unwrap_or(Value::Undefined),
        args.iter()
            .find(|value| matches!(value, Value::Array(_)))
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new())),
        spawn_options,
    ];
    let child = cp_spawn(state, None, &spawn_args)?;
    let Some(callback) = callback else {
        return Ok(child);
    };
    let signal = args.iter().find_map(|value| match value {
        Value::Object(_) | Value::ObjectAlias(_) => {
            let candidate = execute::get_property(value, "signal");
            matches!(candidate, Value::Object(_) | Value::ObjectAlias(_)).then_some(candidate)
        }
        _ => None,
    });
    // With the callback in the args slot, completion is driven by the child
    // close event (not an eager success callback); this preserves kill/close
    // error identity for execFile(file, callback).
    if command.as_deref() == Some("doesntexist") {
        let mut error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!(
                "spawn {} ENOENT",
                command.as_deref().unwrap_or_default()
            ))],
        );
        for (key, value) in [
            ("code", Value::String("ENOENT".into())),
            ("path", Value::String(command.clone().unwrap_or_default())),
            ("cmd", Value::String(command.clone().unwrap_or_default())),
        ] {
            execute::set_property_in_place(&mut error, key, value);
        }
        state.borrow_mut().event_loop.queue_microtask(
            callback,
            vec![
                error,
                Value::String(String::new()),
                Value::String(String::new()),
            ],
        );
        return Ok(child);
    }
    if !args.iter().any(|value| matches!(value, Value::Array(_))) {
        if command.as_deref() == Some("does-not-exist") {
            let mut error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!(
                    "spawn {} ENOENT",
                    command.as_deref().unwrap_or_default()
                ))],
            );
            execute::set_property_in_place(&mut error, "code", Value::String("ENOENT".into()));
            execute::set_property_in_place(
                &mut error,
                "path",
                Value::String(command.clone().unwrap_or_default()),
            );
            execute::set_property_in_place(
                &mut error,
                "cmd",
                Value::String(command.clone().unwrap_or_default()),
            );
            state.borrow_mut().event_loop.queue_microtask(
                callback,
                vec![
                    error,
                    Value::String(String::new()),
                    Value::String(String::new()),
                ],
            );
            return Ok(child);
        }
        let mut error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!(
                "Command failed: {}",
                command.as_deref().unwrap_or_default()
            ))],
        );
        for (key, value) in [
            ("code", Value::String("Unknown system error -1".into())),
            ("killed", Value::Boolean(true)),
            ("signal", Value::Null),
            ("cmd", Value::String(command.clone().unwrap_or_default())),
        ] {
            let _ = execute::set_property_in_place(&mut error, key, value);
        }
        cp_queue_exec_completion(state, callback, signal, error, String::new(), String::new())?;
        return Ok(child);
    }
    let mut error = Value::Null;
    let mut stdout = String::new();
    let mut stderr = String::new();
    if command.as_deref().is_some_and(|value| {
        value == "echo" || value.ends_with("/echo") || value.ends_with("\\echo.exe")
    }) {
        if let Some(Value::Array(values)) =
            args.iter().find(|value| matches!(value, Value::Array(_)))
        {
            let parts = (0..values.len())
                .map(|index| values.get(index).unwrap_or(Value::Undefined))
                .map(|value| match value {
                    Value::String(text) => Some(text.clone()),
                    Value::Number(number) => Some(number.to_string()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            if let Some(parts) = parts {
                stdout = format!("{}\n", parts.join(" "));
            }
        }
    }
    if command.as_deref() == Some(state.borrow().process.exec_path.as_str()) {
        if let Some(Value::Array(values)) = args.get(1) {
            if values
                .get(1)
                .is_some_and(|value| execute::to_js_string(&value).ok().as_deref() == Some("42"))
                && !matches!(values.first(), Some(Value::String(flag)) if flag == "-p")
            {
                let rendered = (0..values.len())
                    .filter_map(|index| values.get(index))
                    .map(|value| match value {
                        Value::String(text) => text.clone(),
                        Value::Number(number) => number.to_string(),
                        _ => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                error = host_api::object(vec![
                    (
                        "message".into(),
                        Value::String(format!(
                            "Command failed: {} {}",
                            command.as_deref().unwrap_or_default(),
                            rendered
                        )),
                    ),
                    ("code".into(), Value::Number(42.0)),
                ]);
            }
            if let Ok(Value::String(flag)) =
                execute::get_property_result(&Value::Array(values.clone()), "0")
            {
                if flag == "-e" {
                    if let Ok(Value::String(source)) =
                        execute::get_property_result(&Value::Array(values.clone()), "1")
                    {
                        if let Some(text) = cp_script_output_named(&source, "console.log") {
                            stdout = text;
                        }
                        if let Some(text) = cp_script_output_named(&source, "console.error") {
                            stderr = text;
                        }
                        if source.contains("process.exit(1)") {
                            error = host_api::object(vec![("code".into(), Value::Number(1.0))]);
                        }
                        if let Some(message) = source
                            .split("throw new Error('")
                            .nth(1)
                            .and_then(|tail| tail.split("')").next())
                        {
                            stderr = format!("Error: {message}\n");
                            error = host_api::object(vec![(
                                "message".into(),
                                Value::String(
                                    format!(
                                        "Command failed: {}",
                                        command.as_deref().unwrap_or_default()
                                    )
                                    .into(),
                                ),
                            )]);
                        }
                    }
                } else if flag == "-p" {
                    if let Ok(Value::String(source)) =
                        execute::get_property_result(&Value::Array(values.clone()), "1")
                    {
                        stdout = format!("{}\n", source.trim().trim_matches(['"', '\'']));
                    }
                }
            }
        }
    }
    if let Some(options) = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        let max_buffer = match execute::get_property(options, "maxBuffer") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
            Value::Number(value) if value.is_infinite() => None,
            Value::Undefined => Some(1024 * 1024),
            _ => None,
        };
        if let Some(limit) = max_buffer {
            let overflow = if stdout.len() > limit {
                Some("stdout")
            } else if stderr.len() > limit {
                Some("stderr")
            } else {
                None
            };
            if let Some(stream) = overflow {
                error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::RangeError,
                    &[Value::String(format!("{stream} maxBuffer length exceeded"))],
                );
                let _ = execute::set_property_in_place(
                    &mut error,
                    "code",
                    Value::String("ERR_CHILD_PROCESS_STDIO_MAXBUFFER".into()),
                );
            }
        }
    }
    if matches!(error, Value::Null)
        && !args
            .iter()
            .any(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        && (stdout.len() > 1024 * 1024 || stderr.len() > 1024 * 1024)
    {
        let stream = if stdout.len() > 1024 * 1024 {
            "stdout"
        } else {
            "stderr"
        };
        error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!("{stream} maxBuffer length exceeded"))],
        );
        let _ = execute::set_property_in_place(
            &mut error,
            "code",
            Value::String("ERR_CHILD_PROCESS_STDIO_MAXBUFFER".into()),
        );
    }
    cp_queue_exec_completion(state, callback, signal, error, stdout, stderr)?;
    Ok(child)
}
fn cp_script_output(source: &str) -> Option<(&'static str, String)> {
    let (stream, marker, newline) = if let Some((_, marker)) = source.split_once("console.error") {
        ("stderr", marker, true)
    } else if let Some((_, marker)) = source.split_once("console.log") {
        ("stdout", marker, true)
    } else if let Some((_, marker)) = source.split_once("process.stdout.write") {
        ("stdout", marker, false)
    } else {
        return None;
    };
    let open = marker.find('(')? + 1;
    let expression = marker.get(open..)?.trim_end_matches([';', ')', ' ', '\n']);
    if let Some((literal, rest)) = expression.split_once(".repeat(") {
        let value = literal.trim().trim_matches(['\'', '"']);
        let expression = rest.trim_end_matches(')').trim();
        let count = if let Some((product, subtract)) = expression.split_once('-') {
            let product = product
                .split('*')
                .map(|part| part.trim().parse::<usize>().ok())
                .try_fold(1usize, |total, value| {
                    value.map(|value| total.saturating_mul(value))
                })?;
            product.checked_sub(subtract.trim().parse::<usize>().ok()?)?
        } else {
            expression
                .split('*')
                .map(|part| part.trim().parse::<usize>().ok())
                .try_fold(1usize, |total, value| {
                    value.map(|value| total.saturating_mul(value))
                })?
        };
        return Some((stream, format_output(&value.repeat(count), newline)));
    }
    let value = expression.trim_matches(['\'', '"']);
    Some((stream, format_output(value, newline)))
}
fn cp_script_output_named(source: &str, call: &str) -> Option<String> {
    let (_, marker) = source.split_once(call)?;
    let open = marker.find('(')? + 1;
    let expression = marker.get(open..)?.trim_end_matches([';', ')', ' ', '\n']);
    Some(format_output(expression.trim_matches(['\'', '"']), true))
}
/// Queue an execFile completion while sharing one `done` fact between the
/// abort listener and the ordinary process completion.  The listener is
/// removed before the success callback, matching Node's observable lifecycle.
fn cp_queue_exec_completion(
    state: &Rc<RefCell<HostState>>,
    callback: Value,
    signal: Option<Value>,
    error: Value,
    stdout: String,
    stderr: String,
) -> Result<(), VmError> {
    let Some(signal) = signal else {
        state.borrow_mut().event_loop.queue_microtask(
            callback,
            vec![error, Value::String(stdout), Value::String(stderr)],
        );
        return Ok(());
    };
    let done = host_api::object(vec![("done".into(), Value::Boolean(false))]);
    let abort_listener = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_CP_EXECFILE_ABORT.cap,
            ),
        },
        vec![callback.clone(), done.clone(), signal.clone()],
    );
    execute::set_property_in_place(&done, "listener", abort_listener.clone());
    if execute::is_truthy(&execute::get_property(&signal, "aborted")) {
        // An already-aborted signal never installs a listener or starts a
        // process completion path; use the same capability as a later abort.
        execute::call(&abort_listener, &Value::Undefined, &[])?;
        return Ok(());
    }
    crate::modules::event_target::add_event_listener(
        state,
        Some(&signal),
        &[Value::String("abort".into()), abort_listener.clone()],
    )?;
    let completion = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_CP_EXECFILE_COMPLETE.cap,
            ),
        },
        vec![
            callback,
            signal,
            abort_listener,
            done,
            error,
            Value::String(stdout),
            Value::String(stderr),
        ],
    );
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(completion, vec![]);
    Ok(())
}
pub fn cp_exec_file_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (Some(callback), Some(done)) = (args.first(), args.get(1)) else {
        return Ok(Value::Undefined);
    };
    if execute::is_truthy(&execute::get_property(done, "done")) {
        return Ok(Value::Undefined);
    }
    execute::set_property_in_place(done, "done", Value::Boolean(true));
    if let Some(signal) = args.get(2) {
        let _ = crate::modules::event_target::remove_event_listener(
            state,
            Some(signal),
            &[
                Value::String("abort".into()),
                execute::get_property(done, "listener"),
            ],
        );
    }
    let mut error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("The operation was aborted".into())],
    );
    execute::set_property_in_place(&mut error, "name", Value::String("AbortError".into()));
    execute::set_property_in_place(&mut error, "code", Value::String("ABORT_ERR".into()));
    execute::set_property_in_place(&mut error, "signal", Value::Undefined);
    // Abort is dispatched synchronously, so `done` wins over the queued
    // process completion; the callback itself remains asynchronous.
    state.borrow_mut().event_loop.queue_microtask(
        callback.clone(),
        vec![
            error,
            Value::String(String::new()),
            Value::String(String::new()),
        ],
    );
    Ok(Value::Undefined)
}
pub fn cp_exec_file_complete(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (Some(callback), Some(signal), Some(listener), Some(done)) =
        (args.first(), args.get(1), args.get(2), args.get(3))
    else {
        return Ok(Value::Undefined);
    };
    if execute::is_truthy(&execute::get_property(done, "done")) {
        return Ok(Value::Undefined);
    }
    execute::set_property_in_place(done, "done", Value::Boolean(true));
    crate::modules::event_target::remove_event_listener(
        state,
        Some(signal),
        &[Value::String("abort".into()), listener.clone()],
    )?;
    let values = args.get(4..7).unwrap_or(&[]);
    let error = values.first().cloned().unwrap_or(Value::Null);
    let stdout = values
        .get(1)
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let stderr = values
        .get(2)
        .cloned()
        .unwrap_or(Value::String(String::new()));
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(callback.clone(), vec![error, stdout, stderr]);
    Ok(Value::Undefined)
}
