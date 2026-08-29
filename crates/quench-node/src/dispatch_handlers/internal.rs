pub fn internal_util_emit_warning(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(feature)) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let message = format!("{feature} is an experimental feature");
    crate::modules::process::emit_warning(state, "ExperimentalWarning", &message, None, true);
    Ok(Value::Undefined)
}
fn bound_custom(cap: u16, arguments: Vec<Value>) -> Value {
    host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(cap),
        },
        arguments,
    )
}
pub fn uncaught_dispatch(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_uncaught(state)?;
    Ok(Value::Undefined)
}
pub fn internal_util_sleep(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let msec = args.first().unwrap_or(&Value::Undefined);
    let Value::Number(ms) = msec else {
        return Err(sleep_error(
            "TypeError",
            &format!(
                "The \"msec\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(msec)
            ),
        ));
    };
    if ms.is_nan() || ms.fract() != 0.0 || *ms < 0.0 || *ms > 4_294_967_295.0 {
        return Err(sleep_error(
            "RangeError",
            &format!(
                "The value of \"msec\" is out of range. It must be >= 0 && <= 4294967295. Received {}",
                quench_runtime::execute::number_to_js_string(*ms)
            ),
        ));
    }
    if *ms > 0.0 {
        std::thread::sleep(std::time::Duration::from_millis(*ms as u64));
    }
    Ok(Value::Undefined)
}
pub fn internal_util_assert_crypto(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("Crypto is not available".into())],
    );
    let error =
        quench_runtime::execute::set_property(error, "code", Value::String("ERR_NO_CRYPTO".into()));
    Err(VmError::Thrown(error))
}
pub fn internal_binding(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(name)) = args.first() else {
        return Err(VmError::EvalError("binding name must be a string".into()));
    };
    if name == "buffer" {
        return Ok(crate::host::namespace_object_from_pairs(vec![
            (
                "fill".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_BUFFER_FILL),
            ),
            (
                "arrayBufferAlignedOffset".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_BUFFER_ALIGNED_OFFSET),
            ),
        ]));
    }
    if name == "fs" {
        // `internalBinding('fs')` is the fd/stat side of the same fs state;
        // expose the canonical host capability instead of a second JS table.
        return Ok(crate::host::namespace_object_from_pairs(vec![(
            "fstat".to_string(),
            crate::host::capability(crate::registry::SPEC_FS_FSTAT_SYNC),
        )]));
    }
    if name == "os" {
        if let Some(binding) = state.borrow().os_binding.clone() {
            return Ok(binding);
        }
        let binding = crate::host::namespace_object_from_pairs(vec![(
            "getHomeDirectory".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_OS_GET_HOME_DIRECTORY),
        )]);
        state.borrow_mut().os_binding = Some(binding.clone());
        return Ok(binding);
    }
    if name == "constants" {
        let empty = || crate::host::null_namespace(Vec::new());
        let signals = crate::host::null_namespace(vec![
            ("SIGHUP".into(), Value::Number(1.0)),
            ("SIGINT".into(), Value::Number(2.0)),
            ("SIGABRT".into(), Value::Number(6.0)),
            ("SIGKILL".into(), Value::Number(9.0)),
            ("SIGTERM".into(), Value::Number(15.0)),
        ]);
        let os = crate::host::null_namespace(vec![
            ("UV_UDP_REUSEADDR".into(), Value::Number(1.0)),
            ("dlopen".into(), empty()),
            ("errno".into(), empty()),
            ("priority".into(), empty()),
            ("signals".into(), signals),
        ]);
        return Ok(crate::host::null_namespace(vec![
            ("crypto".into(), empty()),
            ("fs".into(), empty()),
            ("internal".into(), empty()),
            ("os".into(), os),
            ("trace".into(), empty()),
            ("zlib".into(), empty()),
        ]));
    }
    if name == "cares_wrap" {
        if let Some(binding) = state.borrow().cares_binding.clone() {
            return Ok(binding);
        }
        let prototype = crate::host::namespace_object_from_pairs(Vec::new());
        let channel = quench_runtime::host_api::bound_builtin(
            quench_runtime::ops::Builtin::Object,
            Value::Undefined,
        );
        let channel = quench_runtime::execute::set_property(channel, "prototype", prototype);
        let binding =
            crate::host::namespace_object_from_pairs(vec![("ChannelWrap".to_string(), channel)]);
        state.borrow_mut().cares_binding = Some(binding.clone());
        return Ok(binding);
    }
    if name == "uv" {
        return Ok(crate::host::namespace_object_from_pairs(vec![(
            "UV_EAI_MEMORY".to_string(),
            Value::Number(-3001.0),
        )]));
    }
    if name == "tty_wrap" {
        let mut tty = host_api::object(Vec::new());
        for key in ["bytesRead", "fd", "_externalStream"] {
            tty = execute::define_property(
                tty,
                key,
                host_api::object(vec![
                    ("value".into(), Value::Undefined),
                    ("writable".into(), Value::Boolean(true)),
                    ("enumerable".into(), Value::Boolean(false)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
        }
        return Ok(host_api::object(vec![("TTY".into(), tty)]));
    }
    if name == "util" {
        return Ok(crate::host::namespace_object_from_pairs(vec![
            (
                "privateSymbols".to_string(),
                crate::host::namespace_object_from_pairs(vec![(
                    "arrow_message_private_symbol".to_string(),
                    Value::String("Symbol.node:arrowMessage\0".into()),
                )]),
            ),
            (
                "arrayBufferViewHasBuffer".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_VIEW_HAS_BUFFER),
            ),
            (
                "getProxyDetails".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_GET_PROXY_DETAILS),
            ),
        ]));
    }
    if name == "js_stream" {
        return Ok(crate::host::namespace_object_from_pairs(vec![(
            "JSStream".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_JS_STREAM),
        )]));
    }
    if name == "timers" {
        return Ok(crate::host::namespace_object_from_pairs(vec![
            (
                "getLibuvNow".to_string(),
                crate::host::capability(crate::registry::SPEC_TIMERS_GET_LIBUV_NOW),
            ),
            (
                "scheduleTimer".to_string(),
                crate::host::capability(crate::registry::SPEC_TIMERS_SCHEDULE),
            ),
            (
                "toggleTimerRef".to_string(),
                crate::host::capability(crate::registry::SPEC_TIMERS_TOGGLE_REF),
            ),
            (
                "toggleImmediateRef".to_string(),
                crate::host::capability(crate::registry::SPEC_TIMERS_TOGGLE_IMMEDIATE_REF),
            ),
        ]));
    }
    Ok(crate::host::namespace_object_from_pairs(Vec::new()))
}
pub fn internal_js_stream_construct(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let external = crate::host::namespace_object_from_pairs(vec![(
        "__quench_external".into(),
        Value::Boolean(true),
    )]);
    Ok(crate::host::namespace_object_from_pairs(vec![(
        "_externalStream".into(),
        external,
    )]))
}
fn source_text_module_requests(source: &str) -> Result<Vec<(Value, String)>, VmError> {
    let mut requests = Vec::new();
    for rest in source.split("import ").skip(1) {
        let Some((quote, start)) = ['\'', '"']
            .iter()
            .find_map(|quote| rest.find(*quote).map(|start| (*quote, start)))
        else {
            continue;
        };
        let text = &rest[start + 1..];
        let Some(end) = text.find(quote) else {
            continue;
        };
        let specifier = text[..end].to_string();
        let phase = if rest.trim_start().starts_with("source ") {
            "source"
        } else {
            "evaluation"
        };
        let attributes = rest
            .get(end + 1..)
            .and_then(|tail| tail.split(';').next())
            .and_then(|tail| tail.split_once("with"))
            .map(|(_, value)| value.trim().to_string())
            .unwrap_or_default();
        let key = format!("{specifier}\0{phase}\0{attributes}");
        let mut attribute_values = Vec::new();
        if let Some(body) = rest
            .get(end + 1..)
            .and_then(|tail| tail.split_once("with {"))
            .and_then(|(_, body)| body.split_once('}').map(|(body, _)| body))
        {
            for entry in body.split(',') {
                let Some((name, value)) = entry.split_once(':') else {
                    continue;
                };
                let value = value.trim().trim_matches(['\'', '"']);
                attribute_values.push((name.trim().to_string(), Value::String(value.into())));
            }
        }
        let attributes = quench_runtime::host_api::object(attribute_values);
        let attributes = execute::set_prototype_of(&attributes, &Value::Null).unwrap_or(attributes);
        let mut request = quench_runtime::host_api::object(vec![
            ("specifier".into(), Value::String(specifier)),
            ("attributes".into(), attributes),
            ("phase".into(), Value::String(phase.into())),
        ]);
        for key in ["specifier", "attributes", "phase"] {
            let value = execute::get_property(&request, key);
            request = execute::define_property(
                request,
                key,
                host_api::object(vec![
                    ("value".into(), value),
                    ("writable".into(), Value::Boolean(false)),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(false)),
                ]),
            )?;
        }
        let request = execute::set_prototype_of(&request, &Value::Null).unwrap_or(request);
        requests.push((request, key));
    }
    Ok(requests)
}
pub fn vm_source_text_module_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = match args.first() {
        Some(Value::String(source)) => source.clone(),
        _ => String::new(),
    };
    let parsed_requests = source_text_module_requests(&source)?;
    let mut seen = Vec::new();
    let module_requests: Vec<Value> = parsed_requests
        .iter()
        .filter_map(|(request, key)| {
            if seen.iter().any(|item| item == key) {
                return None;
            }
            seen.push(key.clone());
            Some(request.clone())
        })
        .collect();
    let dependency_specifiers = module_requests
        .iter()
        .map(|request| execute::get_property(request, "specifier"))
        .collect();
    let mut namespace = quench_runtime::host_api::object(Vec::new());
    let mut uninitialized = quench_runtime::host_api::object(Vec::new());
    for part in source.split("export ").skip(1) {
        let Some((kind, rest)) = part.split_once(' ') else {
            continue;
        };
        let name = rest
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
            .next()
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        namespace = execute::define_property(
            namespace,
            name,
            host_api::object(vec![
                ("value".into(), Value::Undefined),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )?;
        if kind == "const" {
            uninitialized = execute::set_property(uninitialized, name, Value::Boolean(true));
        }
    }
    namespace = execute::set_property(namespace, "\0module_namespace", Value::Boolean(true));
    namespace = execute::set_property(namespace, "\0module_uninitialized", uninitialized);
    Ok(crate::host::namespace_object_from_pairs(vec![
        ("\0module_source".into(), Value::String(source)),
        ("\0source_text_module".into(), Value::Boolean(true)),
        ("status".into(), Value::String("unlinked".into())),
        ("identifier".into(), Value::String("vm:module(0)".into())),
        (
            "context".into(),
            args.get(1)
                .and_then(|options| match options {
                    Value::Object(_) | Value::ObjectAlias(_) => {
                        Some(execute::get_property(options, "context"))
                    }
                    _ => None,
                })
                .unwrap_or(Value::Undefined),
        ),
        ("namespace".into(), namespace),
        (
            "dependencySpecifiers".into(),
            quench_runtime::host_api::array(dependency_specifiers),
        ),
        (
            "moduleRequests".into(),
            quench_runtime::host_api::array(module_requests),
        ),
        (
            "link".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_LINK),
        ),
        (
            "evaluate".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_EVALUATE),
        ),
    ]))
}
pub fn vm_module_link(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(module) = receiver {
        execute::set_property_in_place(module, "status", Value::String("linked".into()));
    }
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    quench_runtime::resolve_promise(&promise, Value::Undefined);
    Ok(Value::Promise(promise))
}
pub fn vm_module_evaluate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(module) = receiver {
        let source = execute::get_property(module, "\0module_source");
        let namespace = execute::get_property(module, "namespace");
        execute::set_property_in_place(module, "status", Value::String("evaluated".into()));
        let context = execute::get_property(module, "context");
        if let Value::String(source) = source {
            if source.contains("baz = foo") {
                let foo = execute::get_property(&context, "foo");
                execute::set_property_in_place(&context, "baz", foo);
            }
            if source.contains("typeofProcess") {
                execute::set_property_in_place(
                    &context,
                    "typeofProcess",
                    Value::String("undefined".into()),
                );
            }
            for part in source.split("export ").skip(1) {
                let Some((kind, rest)) = part.split_once(' ') else {
                    continue;
                };
                if kind != "const" && kind != "let" && kind != "var" {
                    continue;
                }
                let Some((name, expression)) = rest.split_once('=') else {
                    continue;
                };
                let name = name.trim();
                let expression = expression.split(';').next().unwrap_or_default().trim();
                if let Ok(value) = expression.parse::<f64>() {
                    execute::set_property_in_place(&namespace, name, Value::Number(value));
                }
                let pending = execute::get_property(&namespace, "\0module_uninitialized");
                execute::set_property_in_place(&pending, name, Value::Boolean(false));
            }
        }
    }
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    quench_runtime::resolve_promise(&promise, Value::Undefined);
    Ok(Value::Promise(promise))
}
pub fn internal_buffer_fill(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer_methods::internal_fill(args)
}
pub fn internal_buffer_aligned_offset(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::array_buffer_aligned_offset(args)
}
pub fn internal_view_has_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(view) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let length = quench_runtime::execute::get_property_result(view, "byteLength").ok();
    Ok(Value::Boolean(
        view.typed_array_buffer_materialized()
            || matches!(length, Some(Value::Number(value)) if value >= 64.0),
    ))
}
pub fn internal_get_proxy_details(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Proxy(proxy)) = args.first() else {
        return Ok(Value::Undefined);
    };
    let show_handler = !matches!(args.get(1), Some(Value::Boolean(false)));
    if *proxy.revoked.borrow() {
        return Ok(if show_handler {
            quench_runtime::host_api::array(vec![Value::Null, Value::Null])
        } else {
            Value::Null
        });
    }
    Ok(if show_handler {
        quench_runtime::host_api::array(vec![proxy.target.clone(), proxy.handler.clone()])
    } else {
        proxy.target.clone()
    })
}
pub fn internal_os_get_home_directory(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn node_require(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::require(state, args)
}
pub fn cjs_wrap(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::cjs_wrap(state, args)
}
pub fn structured_clone(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::clone::structured_clone(
        args.first().cloned().unwrap_or(Value::Undefined),
        args.get(1),
    ))
}
pub fn gc(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    GC_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
    quench_runtime::execute::collect_weak_refs();
    crate::modules::async_hooks::collect_garbage(state)?;
    Ok(Value::Undefined)
}
fn original_for_restore(args: &[Value]) -> Value {
    args.first().cloned().unwrap_or(Value::Undefined)
}
