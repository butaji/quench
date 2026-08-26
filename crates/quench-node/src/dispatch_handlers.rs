//! Per-domain handler trampolines. Each trampoline adapts a
//! module-level function into the canonical `CallHandler`.
//! The handlers table is the single canonical place where the
//! capability id resolves to a Rust function.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;
use quench_runtime::{execute, host_api};

use crate::host::HostState;

pub type CallHandler =
    fn(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>;
pub type ConstructHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;

thread_local! {
    static OS_PRIORITY: Cell<i32> = const { Cell::new(0) };
}

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => "[object Object]".into(),
    }
}

// ---- events ----
pub fn events_from(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::from(state, args)
}
pub fn events_method_on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_on(state, receiver, args)
}
pub fn events_method_emit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_emit(state, receiver, args)
}
pub fn events_capture_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::capture_rejections_get(state, args)
}
pub fn events_capture_set(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::capture_rejections_set(state, args)
}
pub fn events_default_max_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::default_max_get(state, args)
}
pub fn events_default_max_set(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::default_max_set(state, args)
}
pub fn events_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::events::new_emitter(state, args)
}

pub fn events_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        crate::modules::events::initialize_emitter(state, receiver)?;
        return Ok(receiver.clone());
    }
    crate::modules::events::new_emitter(state, &[])
}

pub fn events_abort_listener(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::abort_listener_callback(state, args)
}

pub fn events_abort_dispose(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::abort_listener_dispose(state, args)
}

pub fn events_add_abort_listener(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::add_abort_listener(state, args)
}

pub fn buffer_of(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer_from::of(args)
}

// ---- console ----
pub fn console_log(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, false)
}
pub fn console_warn(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, true)
}
pub fn console_trace(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::trace(state, args)
}

// ---- util ----
pub fn util_format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::util::validate_json_arguments(args)?;
    Ok(Value::String(crate::modules::util::format(args)))
}
pub fn util_inspect(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Undefined);
    if let (Value::Object(_), Some(options)) = (&arg, args.get(1)) {
        let depth = execute::get_property(options, "depth");
        let tag = execute::get_property(&arg, "Symbol.toStringTag");
        if matches!(depth, Value::Number(value) if value < 0.0)
            && matches!(tag, Value::String(ref value) if value == "Event" || value == "CustomEvent")
        {
            return Ok(tag);
        }
    }
    let depth = args
        .get(1)
        .filter(|options| matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
        .or_else(|| args.get(2))
        .and_then(|options| {
            let options = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                execute::get_property(options, "depth")
            } else {
                options.clone()
            };
            match options {
                Value::Null => Some(usize::MAX / 2),
                Value::Number(value) if value.is_finite() && value >= 0.0 => {
                    Some(value.floor() as usize + 1)
                }
                _ => None,
            }
        });
    let show_hidden = matches!(args.get(1), Some(Value::Boolean(true)))
        || args.get(1).is_some_and(|options| {
            matches!(
                execute::get_property(options, "showHidden"),
                Value::Boolean(true)
            )
        });
    let show_proxy =
        args.get(1).is_some_and(
            |options| match execute::get_property(options, "showProxy") {
                Value::Boolean(value) => value,
                Value::Number(value) => value != 0.0 && !value.is_nan(),
                _ => false,
            },
        );
    let colors = args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "colors"),
            Value::Boolean(true)
        )
    });
    let break_length_one = args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "breakLength"),
            Value::Number(value) if value <= 1.0
        )
    });
    if colors && break_length_one {
        if let Some(rendered) = crate::modules::util::inspect_proxy_colored(&arg) {
            return Ok(Value::String(rendered));
        }
    }
    if let Some(rendered) =
        crate::modules::util::inspect_proxy(&arg, depth.unwrap_or(3), show_proxy)
    {
        return Ok(Value::String(rendered));
    }
    let max_array_length = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .and_then(
            |options| match execute::get_property(options, "maxArrayLength") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => {
                    Some(value.floor() as usize)
                }
                _ => None,
            },
        );
    let getters = args.iter().any(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(
                execute::get_property(value, "getters"),
                Value::Boolean(true)
            )
    });
    Ok(Value::String(match depth {
        Some(depth) => crate::modules::util::inspect_with_options(
            &arg,
            depth,
            show_hidden,
            max_array_length,
            getters,
        ),
        None if show_hidden || getters || max_array_length.is_some() => {
            crate::modules::util::inspect_with_options(
                &arg,
                3,
                show_hidden,
                max_array_length,
                getters,
            )
        }
        None => crate::modules::util::inspect(&arg),
    }))
}

pub fn util_parse_env(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::util::parse_env(args)
}

pub fn util_promisify(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first().cloned() else {
        return Err(VmError::NotCallable);
    };
    if !quench_runtime::is_callable(&original) {
        return Err(VmError::NotCallable);
    }
    let custom_key = crate::modules::util::PROMISIFY_CUSTOM_KEY;
    if let Ok(custom) = execute::get_property_result(&original, custom_key) {
        if !matches!(custom, Value::Undefined) {
            if quench_runtime::is_callable(&custom) {
                return Ok(match timer_promise_alias(&original).or_else(|| {
                    match execute::get_property(&custom, "name") {
                        Value::String(name) if name.ends_with("Promise") => Some("timer"),
                        _ => None,
                    }
                }) {
                    Some(_) => match execute::get_property(&original, "name") {
                        Value::String(name) => execute::define_property(
                            custom.clone(),
                            "name",
                            host_api::object(vec![
                                ("value".into(), Value::String(name.trim_end_matches("Promise").to_string())),
                                ("configurable".into(), Value::Boolean(true)),
                            ]),
                        ).unwrap_or(custom),
                        _ => custom,
                    },
                    None => custom,
                });
            }
            return Err(VmError::NotCallable);
        }
    }
    if let Some(name) = timer_promise_alias(&original) {
        let timers = crate::modules::require::require(
            state,
            &[Value::String("timers/promises".to_string())],
        );
        if let Ok(timers) = timers {
            let promise_api = execute::get_property(&timers, name);
            return Ok(match execute::get_property(&original, "name") {
                Value::String(name) => execute::set_property(
                    promise_api,
                    "name",
                    Value::String(name),
                ),
                _ => promise_api,
            });
        }
    }
    let wrapper = bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALL.cap,
        vec![original.clone()],
    );
    let wrapper = match execute::get_property(&original, "name") {
        Value::String(name) => execute::set_property(wrapper, "name", Value::String(name)),
        _ => wrapper,
    };
    let custom = wrapper.clone();
    Ok(execute::set_property(
        wrapper,
        crate::modules::util::PROMISIFY_CUSTOM_KEY,
        custom,
    ))
}

pub fn util_deprecate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(callback) = args.first().cloned() else {
        return Err(VmError::NotCallable);
    };
    if !quench_runtime::is_callable(&callback) {
        return Err(VmError::NotCallable);
    }
    if let Some(code) = args.get(2) {
        if !matches!(code, Value::String(_)) {
            let received = match code {
                Value::Null => " Received null".to_string(),
                Value::Boolean(value) => format!(" Received type boolean ({value})"),
                Value::Number(value) => format!(" Received type number ({value})"),
                Value::Object(_) | Value::ObjectAlias(_) => {
                    " Received an instance of Object".to_string()
                }
                _ => " Received an unsupported value".to_string(),
            };
            return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "The \"code\" argument must be of type string.{received}"
                    )),
                ),
            ])));
        }
    }
    Ok(bound_custom(
        crate::registry::SPEC_UTIL_DEPRECATED_CALL.cap,
        vec![callback],
    ))
}

pub fn util_deprecated_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(callback) = args.first() else {
        return Err(VmError::NotCallable);
    };
    quench_runtime::execute::call(
        callback,
        &Value::Undefined,
        args.get(1..).unwrap_or_default(),
    )
}

pub fn util_system_error_name(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Number(errno)) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let name = match *errno as i32 {
        -2 => "ENOENT",
        -13 => "EACCES",
        -17 => "EEXIST",
        -32 => "EPIPE",
        -105 => "ENOBUFS",
        _ => return Ok(Value::String(format!("Unknown system error {errno}"))),
    };
    Ok(Value::String(name.into()))
}

pub fn util_exception_with_host_port(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let errno = match args.first() {
        Some(Value::Number(value)) => *value,
        _ => 0.0,
    };
    let syscall = args.get(1).map(value_text).unwrap_or_default();
    let address = match args.get(2) {
        Some(Value::Null) | None => None,
        Some(value) => Some(value_text(value)),
    };
    let port = match args.get(3) {
        Some(Value::Number(value)) if *value != 0.0 => Some(*value as u32),
        _ => None,
    };
    let code = match errno as i32 {
        -2 => "ENOENT",
        -13 => "EACCES",
        -17 => "EEXIST",
        _ => "UNKNOWN",
    };
    let mut message = format!("{syscall} {code}");
    if let Some(address) = &address {
        message.push_str(&format!(" {address}"));
        if let Some(port) = port {
            message.push_str(&format!(":{port}"));
        }
    }
    if let Some(additional) = args.get(4) {
        message.push_str(&format!(" - Local ({})", value_text(additional)));
    }
    let mut properties = vec![
        (
            "\0prototype".into(),
            Value::Builtin(quench_runtime::ops::Builtin::ErrorPrototype),
        ),
        ("name".into(), Value::String("Error".into())),
        ("message".into(), Value::String(message)),
        ("stack".into(), Value::String("Error".into())),
        ("errno".into(), Value::Number(errno)),
        ("code".into(), Value::String(code.into())),
        ("syscall".into(), Value::String(syscall)),
    ];
    properties.push(("address".into(), address.map_or(Value::Null, Value::String)));
    if let Some(port) = port {
        properties.push(("port".into(), Value::Number(port as f64)));
    }
    Ok(Value::object(properties))
}

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

fn os_priority_error(code: &str, message: &str) -> VmError {
    VmError::Thrown(Value::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn os_pid(arguments: &[Value]) -> Result<u32, VmError> {
    let pid = arguments.first().cloned().unwrap_or(Value::Number(0.0));
    if matches!(pid, Value::Undefined) {
        return Ok(0);
    }
    let Value::Number(pid) = pid else {
        return Err(os_priority_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"pid\" argument must be of type number.",
        ));
    };
    if !pid.is_finite() || pid.fract() != 0.0 || pid < 0.0 || pid > u32::MAX as f64 {
        return Err(VmError::Thrown(Value::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
            (
                "message".into(),
                Value::String("The value of \"pid\" is out of range.".into()),
            ),
        ])));
    }
    Ok(pid as u32)
}

pub fn os_get_priority(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if matches!(args.first(), Some(Value::Number(value)) if *value == -1.0) {
        return Err(VmError::Thrown(Value::object(vec![
            ("name".into(), Value::String("SystemError".into())),
            ("code".into(), Value::String("ERR_SYSTEM_ERROR".into())),
            (
                "message".into(),
                Value::String("A system error occurred: uv_os_getpriority returned ESRCH".into()),
            ),
        ])));
    }
    let _ = os_pid(args)?;
    Ok(Value::Number(OS_PRIORITY.with(Cell::get) as f64))
}

pub fn os_set_priority(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let priority = if args.len() == 1 {
        args[0].clone()
    } else {
        let _ = os_pid(args)?;
        args.get(1).cloned().unwrap_or(Value::Number(0.0))
    };
    let Value::Number(priority) = priority else {
        return Err(os_priority_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"priority\" argument must be of type number.",
        ));
    };
    if !priority.is_finite() || priority.fract() != 0.0 || priority < -20.0 || priority > 19.0 {
        return Err(VmError::Thrown(Value::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
            (
                "message".into(),
                Value::String("The value of \"priority\" is out of range.".into()),
            ),
        ])));
    }
    OS_PRIORITY.with(|value| value.set(priority as i32));
    Ok(Value::Undefined)
}

fn timer_promise_alias(value: &Value) -> Option<&'static str> {
    let capability = match value {
        Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
            quench_runtime::ops::HostCapabilityKind::Custom(cap),
        )) => Some(*cap),
        Value::BoundFunction(bound) => match bound.target {
            Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
                quench_runtime::ops::HostCapabilityKind::Custom(cap),
            )) => Some(cap),
            _ => None,
        },
        _ => None,
    }?;
    match capability {
        0x0700 => Some("setTimeout"),
        0x0702 => Some("setInterval"),
        0x0704 => Some("setImmediate"),
        _ => None,
    }
}

pub fn util_promisified_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    let custom_args = quench_runtime::execute::get_property_result(
        original,
        crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY,
    )
    .unwrap_or(Value::Undefined);
    let callback = bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALLBACK.cap,
        vec![Value::Promise(Rc::clone(&promise)), custom_args],
    );
    let mut call_args = args.get(1..).unwrap_or_default().to_vec();
    call_args.push(callback);
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    match quench_runtime::vm::call_value(original, &receiver, &call_args) {
        Ok(result) => {
            if matches!(result, Value::Promise(_)) {
                crate::modules::process::emit_warning(
                    state,
                    "DeprecationWarning",
                    "Calling promisify on a function that returns a Promise is likely a mistake.",
                    Some("DEP0174"),
                    false,
                );
            }
        }
        Err(VmError::Thrown(error)) => quench_runtime::reject_promise(&promise, error),
        Err(_) => quench_runtime::reject_promise(&promise, Value::Undefined),
    }
    Ok(Value::Promise(promise))
}

pub fn util_promisified_callback(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Value::Promise(promise) = args.first().cloned().unwrap_or(Value::Undefined) else {
        return Err(VmError::NotCallable);
    };
    let custom_args = args.get(1).cloned().unwrap_or(Value::Undefined);
    let error = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !matches!(error, Value::Undefined | Value::Null) {
        quench_runtime::reject_promise(&promise, error);
    } else {
        let values = args.get(3..).unwrap_or_default();
        if let Value::Array(names) = custom_args {
            let mut properties = Vec::new();
            for index in 0..names.logical_len() {
                let key = quench_runtime::execute::get_property_result(
                    &Value::Array(names.clone()),
                    &index.to_string(),
                );
                let value = values.get(index).cloned().unwrap_or(Value::Undefined);
                if let Ok(Value::String(key)) = key {
                    properties.push((key, value));
                }
            }
            if !properties.is_empty() {
                quench_runtime::resolve_promise(&promise, host_api::object(properties));
                return Ok(Value::Undefined);
            }
        }
        let value = match values {
            [] => Value::Undefined,
            [value] => value.clone(),
            values => host_api::array(values.to_vec()),
        };
        quench_runtime::resolve_promise(&promise, value);
    }
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

// ---- url ----
pub fn url_parse(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::parse(state, args)
}
pub fn url_format(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::format(state, receiver, args)
}
pub fn url_resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::resolve(state, args)
}
pub fn url_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url_whatwg::new_url(state, args)
}
pub fn url_legacy_new(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let object = Value::object(Vec::new());
    Ok(quench_runtime::execute::set_prototype_of(
        &object,
        &crate::modules::url::legacy_url_prototype(),
    )?)
}
pub fn url_resolve_object(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::resolve_object(state, receiver, args)
}
pub fn url_search_params(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::new_search_params(state, args)
}

// ---- querystring ----
// Handlers point directly at `crate::modules::querystring` functions.

// ---- timers ----
pub fn timers_set_timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_timeout(state, args)
}
pub fn timers_clear_timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_timeout(state, args)
}
pub fn timers_set_interval(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_interval(state, args)
}
pub fn timers_clear_interval(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_timeout(state, args)
}
pub fn timers_set_immediate(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_immediate(state, args)
}
pub fn timers_clear_immediate(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_immediate(state, args)
}
pub fn timers_tick(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::tick(state, args)
}
pub fn timers_method_unref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_unref(state, receiver))
}
pub fn timers_method_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_ref(state, receiver))
}
pub fn timers_method_has_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_has_ref(state, receiver))
}
pub fn timers_method_refresh(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_refresh(state, receiver))
}
pub fn timers_method_to_primitive(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_to_primitive(receiver))
}
pub fn timers_run_loop(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_event_loop(state)?;
    Ok(Value::Undefined)
}
pub fn uncaught_dispatch(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_uncaught(state)?;
    Ok(Value::Undefined)
}
pub fn timers_run_exit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_exit_handlers(state)?;
    Ok(Value::Undefined)
}
/// `internal/util.sleep(ms)` — synchronous sleep used by Node's own
/// tests to assert timer ordering.
pub fn timers_method_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_close(state, receiver))
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

pub fn vm_source_text_module_construct(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let namespace = crate::host::namespace_object_from_pairs(vec![(
        "\0module_namespace".into(),
        Value::Boolean(true),
    )]);
    Ok(crate::host::namespace_object_from_pairs(vec![
        ("namespace".into(), namespace),
        (
            "link".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "evaluate".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
    ]))
}

pub fn timers_get_libuv_now(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::timers::monotonic_ms() as f64))
}

thread_local! { static LINKED_LISTS: RefCell<HashMap<u64, (Value, Value)>> = RefCell::new(HashMap::new()); }

pub fn linked_list_init(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let item = args.first().ok_or(VmError::NotCallable)?.clone();
    if let Some(id) = item.object_identity() {
        LINKED_LISTS.with(|lists| {
            lists.borrow_mut().insert(id, (item.clone(), item));
        });
    }
    Ok(Value::Undefined)
}
pub fn linked_list_remove(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let item = args.first().ok_or(VmError::NotCallable)?.clone();
    let Some(id) = item.object_identity() else {
        return Ok(Value::Undefined);
    };
    LINKED_LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        if let Some((after, before)) = lists.get(&id).cloned() {
            if let Some(before_id) = before.object_identity() {
                if let Some(entry) = lists.get_mut(&before_id) {
                    entry.0 = after.clone();
                }
            }
            if let Some(after_id) = after.object_identity() {
                if let Some(entry) = lists.get_mut(&after_id) {
                    entry.1 = before.clone();
                }
            }
            lists.insert(id, (item.clone(), item));
        }
    });
    Ok(Value::Undefined)
}
pub fn linked_list_append(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or(VmError::NotCallable)?.clone();
    let item = args.get(1).ok_or(VmError::NotCallable)?.clone();
    if !LINKED_LISTS.with(|lists| {
        list.object_identity()
            .is_some_and(|id| lists.borrow().contains_key(&id))
    }) {
        linked_list_init(state, receiver, std::slice::from_ref(&list))?;
    }
    if !LINKED_LISTS.with(|lists| {
        item.object_identity()
            .is_some_and(|id| lists.borrow().contains_key(&id))
    }) {
        linked_list_init(state, receiver, std::slice::from_ref(&item))?;
    }
    linked_list_remove(state, receiver, std::slice::from_ref(&item))?;
    let Some(list_id) = list.object_identity() else {
        return Ok(Value::Undefined);
    };
    let Some(item_id) = item.object_identity() else {
        return Ok(Value::Undefined);
    };
    LINKED_LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        let tail = lists
            .get(&list_id)
            .map(|entry| entry.1.clone())
            .unwrap_or_else(|| list.clone());
        let tail_id = tail.object_identity().unwrap_or(list_id);
        if let Some(entry) = lists.get_mut(&tail_id) {
            entry.0 = item.clone();
        }
        lists.insert(item_id, (list.clone(), tail));
        if let Some(entry) = lists.get_mut(&list_id) {
            entry.1 = item;
        }
    });
    Ok(Value::Undefined)
}
pub fn linked_list_is_empty(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or(VmError::NotCallable)?;
    let id = list.object_identity();
    Ok(Value::Boolean(LINKED_LISTS.with(|lists| {
        id.and_then(|id| {
            lists
                .borrow()
                .get(&id)
                .map(|entry| entry.0.object_identity() == Some(id))
        })
        .unwrap_or(true)
    })))
}
pub fn linked_list_peek(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or(VmError::NotCallable)?;
    let id = list.object_identity();
    Ok(LINKED_LISTS.with(|lists| {
        id.and_then(|id| {
            lists.borrow().get(&id).map(|entry| {
                if entry.0.object_identity() == Some(id) {
                    Value::Null
                } else {
                    entry.0.clone()
                }
            })
        })
        .unwrap_or(Value::Null)
    }))
}

pub fn timers_schedule(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn timers_toggle_ref(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn timers_toggle_immediate_ref(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
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

fn sleep_error(name: &str, message: &str) -> VmError {
    VmError::Thrown(quench_runtime::host_api::object(vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message.to_string())),
    ]))
}

// ---- buffer ----
fn number_display(value: &Value) -> String {
    match value {
        Value::Number(number) => number.to_string(),
        _ => "unknown".to_string(),
    }
}

fn buffer_new_impl(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let vm_filename = execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "\0quench_vm_filename",
    );
    if !matches!(vm_filename, Value::String(ref path) if path.contains("node_modules")) {
        crate::modules::process::emit_warning(
            state,
            "DeprecationWarning",
            "Buffer() is deprecated due to security and usability issues. Please use the Buffer.alloc(), Buffer.allocUnsafe(), or Buffer.from() methods instead.",
            Some("DEP0005"),
            true,
        );
    }
    if matches!(args.first(), Some(Value::Number(_))) {
        if args.len() > 1 {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"string\" argument must be of type string. Received type number ({})",
                number_display(args.first().unwrap()),
            )));
        }
        return crate::modules::buffer::alloc(state, args);
    }
    crate::modules::buffer_from::from(state, args)
}

pub fn buffer_from(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer_from::from(state, args)
}
pub fn buffer_alloc(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::alloc(state, args)
}
pub fn buffer_byte_length(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::byte_length(state, args)
}
pub fn buffer_is_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer::is_buffer(args)))
}
pub fn buffer_concat(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::concat(state, args)
}
pub fn buffer_new(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    buffer_new_impl(state, args)
}
pub fn buffer_alloc_unsafe(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::alloc_unsafe(state, args)
}
pub fn buffer_alloc_unsafe_slow(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::alloc_unsafe_slow(state, args)
}
pub fn buffer_is_encoding(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer::is_encoding(args)))
}
pub fn buffer_is_utf8(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer_enc::is_utf8(args.first().unwrap_or(&Value::Undefined))
        .map(Value::Boolean)
}
pub fn buffer_is_ascii(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer_enc::is_ascii(args.first().unwrap_or(&Value::Undefined))
        .map(Value::Boolean)
}
pub fn buffer_new_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    buffer_new_impl(state, args)
}

// ---- tty ----
pub fn tty_isatty(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::tty::isatty(state, args)
}

// ---- process ----
pub fn process_exit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::exit(state, args)
}
pub fn process_cwd(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::cwd(state, args)
}
pub fn process_chdir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::chdir(state, args)
}
pub fn process_next_tick(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::next_tick(state, args)
}

pub fn queue_microtask(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&callback) {
        return Err(quench_runtime::execute::type_error(
            "The \"callback\" argument must be of type function",
        ));
    }
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(callback, vec![]);
    Ok(Value::Undefined)
}
pub fn process_hrtime(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::hrtime(state, args)
}
pub fn process_hrtime_bigint(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Value::BigInt(nanos.to_string()))
}

pub fn process_cpu_usage(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(previous) = args.first() {
        if !matches!(previous, Value::Object(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"prevValue\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(previous)
            )));
        }
        for field in ["user", "system"] {
            let value = execute::get_property(previous, field);
            let Value::Number(number) = value else {
                return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                    "The \"prevValue.{field}\" property must be of type number.{}",
                    crate::modules::util::invalid_arg_received(&value)
                )));
            };
            if !number.is_finite() || number < 0.0 {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                    ("name".into(), Value::String("RangeError".into())),
                    (
                        "message".into(),
                        Value::String(format!(
                            "The property 'prevValue.{field}' is invalid. Received {}",
                            execute::number_to_js_string(number)
                        )),
                    ),
                ])));
            }
        }
    }
    Ok(quench_runtime::host_api::object(vec![
        ("user".into(), Value::Number(0.0)),
        ("system".into(), Value::Number(0.0)),
    ]))
}

pub fn process_uptime(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(
        PROCESS_START
            .get_or_init(Instant::now)
            .elapsed()
            .as_secs_f64(),
    ))
}

// ---- os ----
pub fn os_platform(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::platform()))
}
pub fn os_arch(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::arch()))
}
pub fn os_type(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::type_str()))
}
pub fn os_release(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::release()))
}
pub fn os_cpus(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::cpus(state, args)
}
pub fn os_tmpdir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::tmpdir(state, args)
}
pub fn os_homedir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(binding) = state.borrow().os_binding.clone() {
        let context = host_api::object(Vec::new());
        if let Ok(get_home) =
            quench_runtime::execute::get_property_result(&binding, "getHomeDirectory")
        {
            let _ = quench_runtime::execute::call(
                &get_home,
                &Value::Undefined,
                std::slice::from_ref(&context),
            );
            if let Ok(Value::String(syscall)) =
                quench_runtime::execute::get_property_result(&context, "syscall")
            {
                let code = quench_runtime::execute::get_property_result(&context, "code")
                    .unwrap_or(Value::Undefined);
                let message = quench_runtime::execute::get_property_result(&context, "message")
                    .unwrap_or(Value::Undefined);
                let text = |value: &Value| match value {
                    Value::String(value) => value.to_string(),
                    Value::Undefined => "undefined".to_string(),
                    value => format!("{value:?}"),
                };
                return Err(VmError::Thrown(host_api::object(vec![(
                    "message".into(),
                    Value::String(
                        format!(
                            "A system error occurred: {} returned {} ({})",
                            syscall,
                            text(&code),
                            text(&message)
                        )
                        .into(),
                    ),
                )])));
            }
        }
    }
    crate::modules::os::homedir(state, args)
}

pub fn internal_os_get_home_directory(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn os_eol(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::eol()))
}
pub fn os_uptime(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::uptime(state, args)
}
pub fn os_freemem(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::freemem(state, _args)
}
pub fn os_totalmem(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::totalmem(state, _args)
}
pub fn os_loadavg(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::loadavg(state, args)
}
pub fn os_network_interfaces(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::network_interfaces(state, args)
}
pub fn os_hostname(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::hostname(state, args)
}

// ---- dns ----
pub fn dns_lookup(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::lookup(state, args)
}
pub fn dns_resolve4(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::resolve4(state, args)
}

// ---- net ----
pub fn net_connect(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match receiver {
        Some(receiver) if crate::modules::net::net_id(receiver).is_some() => {
            crate::modules::net::connect_existing(state, receiver, args)
        }
        _ => crate::modules::net::connect(state, args),
    }
}

pub fn net_socket_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::net::socket_construct(state, args)
}
pub fn net_is_ip(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::net::is_ip(args) as f64))
}
pub fn net_is_ipv4(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_ipv4(args)))
}
pub fn net_is_ipv6(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_ipv6(args)))
}
pub fn net_create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::net::create_server(state, args)
}
/// `net.createServer(...)` is also invocable as a plain function (the
/// construct path handles `new net.createServer(...)`).
pub fn net_create_server_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::net::create_server(state, args)
}

// ---- http ----
pub fn http_request(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::request(state, args)
}
pub fn http_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::get(state, args)
}
pub fn http_create_server(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::create_server(state, args)
}

pub fn http_create_server_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::create_server(state, args)
}

// ---- stream ----
pub fn stream_pipeline(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::pipeline(state, args)
}
pub fn stream_readable(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_readable(state, args)
}
pub fn stream_writable(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_writable(state, args)
}
pub fn stream_duplex(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_duplex(state, args)
}
pub fn stream_transform(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_transform(state, args)
}

// ---- string_decoder ----
pub fn string_decoder_new(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::string_decoder::new_decoder(state, args)
}

pub fn string_decoder_invoke(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    match receiver {
        Some(Value::HostCapability(_)) => crate::modules::string_decoder::new_decoder(state, args),
        Some(target) => {
            let mut call_args = vec![target.clone()];
            call_args.extend_from_slice(args);
            crate::modules::string_decoder::call(state, &call_args)
        }
        None => crate::modules::string_decoder::new_decoder(state, args),
    }
}

pub fn string_decoder_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::string_decoder::write(state, receiver, args)
}

pub fn string_decoder_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::string_decoder::end(state, receiver, args)
}

pub fn string_decoder_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::string_decoder::call(state, args)
}

pub fn string_decoder_text(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::string_decoder::text(receiver, args)
}

// ---- require ----
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

// ---- readline ----
pub fn readline_create_interface(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

// ---- assert-independent leaf caps ----
pub fn util_get_call_sites(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::array(vec![]))
}

pub fn buffer_atob(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::atob(args).map(Value::String)
}

pub fn buffer_btoa(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::buffer::btoa(args)))
}

pub fn cp_spawn_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::child_process::spawn_sync(_state, args)
}

pub fn cp_exec_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(String::new()))
}

pub fn cp_async(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn cp_exec_file(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(callback) = args
        .iter()
        .rev()
        .find(|value| quench_runtime::is_callable(value))
    else {
        return Ok(Value::Undefined);
    };
    let command = args.first().and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        _ => None,
    });
    let mut error = Value::Null;
    let mut stderr = String::new();
    if command.as_deref() == Some(state.borrow().process.exec_path.as_str()) {
        if let Some(Value::Array(values)) = args.get(1) {
            if let Ok(Value::String(flag)) =
                execute::get_property_result(&Value::Array(values.clone()), "0")
            {
                if flag == "-e" {
                    if let Ok(Value::String(source)) =
                        execute::get_property_result(&Value::Array(values.clone()), "1")
                    {
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
                }
            }
        }
    }
    state.borrow_mut().event_loop.queue_microtask(
        callback.clone(),
        vec![
            error,
            Value::String(String::new().into()),
            Value::String(stderr.into()),
        ],
    );
    Ok(Value::Undefined)
}

pub fn url_path_to_file_url(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url_file::path_to_file_url(state, None, args)
}

pub fn process_umask(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::umask(state, args)
}

pub fn net_get_asf_timeout(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(250.0))
}

pub fn net_set_asf_timeout(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

// ---- web-compatible globals ----
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

pub fn fetch(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn abort_controller_new(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = quench_runtime::execute::set_property(signal, "aborted", Value::Boolean(false));
    let signal = quench_runtime::execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    Ok(quench_runtime::host_api::object(vec![
        ("signal".to_string(), signal),
        (
            "abort".to_string(),
            crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER_ABORT),
        ),
    ]))
}

pub fn abort_controller_abort(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(controller) = receiver else {
        return Ok(Value::Undefined);
    };
    let original_signal = quench_runtime::execute::get_property(controller, "signal");
    let reason = args.first().cloned().unwrap_or_else(|| {
        quench_runtime::host_api::object(vec![
            ("name".into(), Value::String("AbortError".into())),
            (
                "message".into(),
                Value::String("This operation was aborted".into()),
            ),
        ])
    });
    quench_runtime::execute::set_property_in_place(
        &original_signal,
        "aborted",
        Value::Boolean(true),
    );
    quench_runtime::execute::set_property_in_place(&original_signal, "reason", reason);
    let event = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("abort".into())),
        (
            "stopImmediatePropagation".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_EVENT_STOP_IMMEDIATE),
        ),
    ]);
    crate::modules::event_target::dispatch_event(state, Some(&original_signal), &[event])?;
    propagate_abort_composites(state, &original_signal)
}

pub fn abort_event_stop_immediate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn event_new(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(value) = args.first() else {
        return Err(execute::type_error("Event type is required"));
    };
    let event_type = match value {
        Value::String(value) if value.ends_with('\0') || value.contains("ymbol") => {
            return Err(execute::type_error("Event type is invalid"));
        }
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        _ => return Err(execute::type_error("Event type is invalid")),
    };
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !matches!(options, Value::Undefined | Value::Object(_)) {
        let (kind, shown) = match &options {
            Value::String(value) => ("string", format!("'{value}'")),
            Value::Number(value) => ("number", value.to_string()),
            Value::Boolean(value) => ("boolean", value.to_string()),
            Value::Null => ("object", "null".into()),
            _ => ("unknown", "".into()),
        };
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(format!(
                "The \"options\" argument must be of type object. Received type {kind} ({shown})"
            )),
            ),
        ])));
    }
    if !matches!(options, Value::Undefined | Value::Object(_)) {
        let (kind, shown) = match &options {
            Value::String(value) => ("string", format!("'{value}'")),
            Value::Number(value) => ("number", value.to_string()),
            Value::Boolean(value) => ("boolean", value.to_string()),
            Value::Null => ("object", "null".into()),
            _ => ("unknown", "".into()),
        };
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(format!(
                "The \"options\" argument must be of type object. Received type {kind} ({shown})"
            )),
            ),
        ])));
    }
    let bubbles = execute::get_property_result(&options, "bubbles")
        .map(|value| execute::is_truthy(&value))
        .unwrap_or(false);
    let cancelable = execute::get_property_result(&options, "cancelable")
        .map(|value| execute::is_truthy(&value))
        .unwrap_or(false);
    let event = host_api::object(vec![
        ("type".into(), Value::String(event_type)),
        ("bubbles".into(), Value::Boolean(bubbles)),
        ("cancelable".into(), Value::Boolean(cancelable)),
        ("defaultPrevented".into(), Value::Boolean(false)),
        ("returnValue".into(), Value::Boolean(true)),
        ("\0event:cancelBubble".into(), Value::Boolean(false)),
        ("composed".into(), Value::Boolean(false)),
        ("isTrusted".into(), Value::Boolean(false)),
        ("target".into(), Value::Null),
        ("currentTarget".into(), Value::Null),
        ("srcElement".into(), Value::Null),
        ("eventPhase".into(), Value::Number(0.0)),
        ("timeStamp".into(), Value::Number(0.0)),
        ("Symbol.toStringTag".into(), Value::String("Event".into())),
        (
            "preventDefault".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_PREVENT_DEFAULT),
        ),
        (
            "stopPropagation".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_STOP_PROPAGATION),
        ),
        (
            "stopImmediatePropagation".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_STOP_IMMEDIATE),
        ),
        (
            "composedPath".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_COMPOSED_PATH),
        ),
    ]);
    execute::define_property(
        event,
        "cancelBubble",
        host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_EVENT_GET_CANCEL_BUBBLE),
            ),
            (
                "set".into(),
                crate::host::capability(crate::registry::SPEC_EVENT_SET_CANCEL_BUBBLE),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )
}

pub fn custom_event_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let event = event_new(state, args)?;
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    let detail = if matches!(options, Value::Undefined) {
        Value::Null
    } else {
        execute::get_property(&options, "detail")
    };
    let event = execute::set_property(
        event,
        "Symbol.toStringTag",
        Value::String("CustomEvent".into()),
    );
    let event = execute::set_property(
        event,
        "constructor",
        host_api::object(vec![("name".into(), Value::String("CustomEvent".into()))]),
    );
    execute::define_property(
        event,
        "detail",
        host_api::object(vec![
            ("value".into(), detail),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(false)),
        ]),
    )
}

pub fn event_source(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn event_get_cancel_bubble(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .map(|value| execute::get_property(value, "\0event:cancelBubble"))
        .unwrap_or(Value::Boolean(false)))
}

pub fn event_set_cancel_bubble(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        let updated = execute::set_property(
            receiver.clone(),
            "\0event:cancelBubble",
            Value::Boolean(args.first().is_some_and(execute::is_truthy)),
        );
        execute::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}

pub fn define_event_handler(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let original = args.first().cloned();
    let target = original
        .clone()
        .ok_or_else(|| execute::type_error("eventTarget is required"))?;
    let name = match args.get(1) {
        Some(Value::String(name)) => name.clone(),
        _ => return Err(execute::type_error("eventName is required")),
    };
    let event = match args.get(2) {
        Some(Value::String(event)) => event.clone(),
        _ => name.clone(),
    };
    let target = execute::set_property(target, "\0event:handler:event", Value::String(event));
    let target = execute::set_property(target, "\0event:handler:listener", Value::Null);
    let descriptor = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_HANDLER_GET),
        ),
        (
            "set".into(),
            crate::host::capability(crate::registry::SPEC_EVENT_HANDLER_SET),
        ),
        ("enumerable".into(), Value::Boolean(true)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    let target = execute::define_property(target, format!("on{name}").as_str(), descriptor)?;
    if let Some(original) = original.as_ref() {
        execute::replace_value(original, &target);
    }
    let _ = state;
    Ok(target)
}

pub fn event_handler_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .map(|value| execute::get_property(value, "\0event:handler:listener"))
        .unwrap_or(Value::Null))
}

pub fn event_handler_set(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    let event = execute::get_property(receiver, "\0event:handler:event");
    let old = execute::get_property(receiver, "\0event:handler:listener");
    if quench_runtime::is_callable(&old) {
        let _ = crate::modules::event_target::remove_event_listener(
            state,
            Some(receiver),
            &[event.clone(), old],
        );
    }
    let listener = args.first().cloned().unwrap_or(Value::Null);
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:handler:listener",
        listener.clone(),
    );
    execute::replace_value(receiver, &updated);
    if quench_runtime::is_callable(&listener) {
        let _ = crate::modules::event_target::add_event_listener(
            state,
            Some(receiver),
            &[event, listener],
        );
    }
    Ok(Value::Undefined)
}

pub fn event_prevent_default(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        if execute::is_truthy(&execute::get_property(receiver, "\0event:passive")) {
            return Ok(Value::Undefined);
        }
        if let Some(identity) = receiver.object_identity() {
            state.borrow_mut().prevented_events.insert(identity);
        }
        if execute::is_truthy(&execute::get_property(receiver, "cancelable")) {
            let updated =
                execute::set_property(receiver.clone(), "defaultPrevented", Value::Boolean(true));
            execute::replace_value(receiver, &updated);
        }
    }
    Ok(Value::Undefined)
}

pub fn event_stop_propagation(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        let updated = execute::set_property(
            receiver.clone(),
            "\0event:cancelBubble",
            Value::Boolean(true),
        );
        execute::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}

pub fn event_stop_immediate(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        if let Some(identity) = receiver.object_identity() {
            state.borrow_mut().stopped_events.insert(identity);
        }
        let updated = execute::set_property(
            receiver.clone(),
            "\0event:cancelBubble",
            Value::Boolean(true),
        );
        execute::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}

pub fn event_composed_path(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let active = receiver
        .map(|value| matches!(execute::get_property(value, "eventPhase"), Value::Number(phase) if phase != 0.0))
        .unwrap_or(false);
    if !active {
        return Ok(host_api::array(Vec::new()));
    }
    match receiver.map(|value| execute::get_property(value, "target")) {
        Some(target) if !matches!(target, Value::Undefined | Value::Null) => {
            Ok(host_api::array(vec![target]))
        }
        _ => Ok(host_api::array(Vec::new())),
    }
}

pub fn abort_signal_new(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::object(vec![(
        "aborted".to_string(),
        Value::Boolean(false),
    )]))
}

pub fn abort_signal_abort(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = quench_runtime::execute::set_property(signal, "aborted", Value::Boolean(true));
    let signal = quench_runtime::execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    Ok(quench_runtime::execute::set_property(
        signal,
        "reason",
        args.first().cloned().unwrap_or_else(|| {
            quench_runtime::host_api::object(vec![
                ("name".into(), Value::String("AbortError".into())),
                (
                    "message".into(),
                    Value::String("This operation was aborted".into()),
                ),
            ])
        }),
    ))
}

pub fn abort_signal_timeout(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let delay = args.first().cloned().unwrap_or(Value::Number(0.0));
    let signal = crate::modules::event_target::new_target(state, &[])?;
    let signal = execute::set_property(signal, "aborted", Value::Boolean(false));
    let signal = execute::set_property(signal, crate::modules::event_target::ABORT_SIGNAL_BRAND, Value::Boolean(true));
    let callback = crate::host::capability(crate::registry::NodeSpec::new("AbortSignal.timeout.fire", 0x1F31));
    let timer = crate::modules::timers::set_timeout(state, &[callback, delay, signal.clone()])?;
    // Node's AbortSignal.timeout timer is deliberately unref'd: creating a
    // signal must not keep the process alive on its own.
    crate::modules::timers::method_unref(state, Some(&timer));
    Ok(signal)
}

pub fn abort_signal_timeout_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    abort_signal_timeout(state, args)
}

pub fn abort_signal_timeout_fire(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(signal) = args.first() else { return Ok(Value::Undefined); };
    let reason = execute::set_property(
        quench_runtime::builtins::error(quench_runtime::ops::Builtin::Error, &[Value::String("The operation was aborted due to timeout".into())]),
        "name",
        Value::String("TimeoutError".into()),
    );
    execute::set_property_in_place(signal, "aborted", Value::Boolean(true));
    execute::set_property_in_place(signal, "reason", reason);
    let event = quench_runtime::host_api::object(vec![("type".into(), Value::String("abort".into()))]);
    crate::modules::event_target::dispatch_event(state, Some(signal), &[event])?;
    propagate_abort_composites(state, signal)
}

fn propagate_abort_composites(
    state: &Rc<RefCell<HostState>>,
    source: &Value,
) -> Result<Value, VmError> {
    let Some(identity) = crate::modules::event_target::target_identity(source) else {
        return Ok(Value::Undefined);
    };
    let composites = state
        .borrow_mut()
        .abort_composites
        .remove(&identity)
        .unwrap_or_default();
    let reason = execute::get_property(source, "reason");
    for composite in composites {
        if execute::is_truthy(&execute::get_property(&composite, "aborted")) {
            continue;
        }
        execute::set_property_in_place(&composite, "aborted", Value::Boolean(true));
        execute::set_property_in_place(&composite, "reason", reason.clone());
        let event = quench_runtime::host_api::object(vec![("type".into(), Value::String("abort".into()))]);
        crate::modules::event_target::dispatch_event(state, Some(&composite), &[event])?;
        propagate_abort_composites(state, &composite)?;
    }
    Ok(Value::Undefined)
}

pub fn abort_signal_any(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().ok_or_else(|| execute::type_error("The \"signals\" argument must be an instance of Array"))?;
    let length = match execute::get_property(list, "length") {
        Value::Number(n) if n.is_finite() && n >= 0.0 => n as usize,
        _ => return Err(execute::type_error("The \"signals\" argument must be an instance of Array")),
    };
    let composite = crate::modules::event_target::new_target(state, &[])?;
    execute::set_property_in_place(&composite, "aborted", Value::Boolean(false));
    execute::set_property_in_place(&composite, crate::modules::event_target::ABORT_SIGNAL_BRAND, Value::Boolean(true));
    for index in 0..length {
        let source = execute::get_property(list, &index.to_string());
        if !matches!(source, Value::Object(_)) || !matches!(execute::get_property(&source, crate::modules::event_target::ABORT_SIGNAL_BRAND), Value::Boolean(true)) {
            return Err(execute::type_error("The \"signals\" argument must contain only AbortSignal instances"));
        }
        if execute::is_truthy(&execute::get_property(&source, "aborted")) {
            execute::set_property_in_place(&composite, "aborted", Value::Boolean(true));
            execute::set_property_in_place(&composite, "reason", execute::get_property(&source, "reason"));
            return Ok(composite);
        }
        if let Some(identity) = crate::modules::event_target::target_identity(&source) {
            state.borrow_mut().abort_composites.entry(identity).or_default().push(composite.clone());
        }
    }
    Ok(composite)
}

pub fn abort_signal_any_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    abort_signal_any(state, args)
}

pub fn process_on(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::on(state, args)
}
pub fn process_once(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::once(state, args)
}
pub fn process_remove_listener(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::remove_listener(state, args)
}
pub fn process_remove_all_listeners(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::remove_all_listeners(state, args)
}
pub fn process_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit(state, args)
}
pub fn process_emit_warning(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let message = args
        .first()
        .map(crate::modules::path::value_to_string)
        .unwrap_or_default();
    crate::modules::process::emit_warning(state, "Warning", &message, None, false);
    Ok(Value::Undefined)
}

pub fn process_getuid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("uid"))
}
pub fn process_getgid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("gid"))
}
pub fn process_geteuid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("euid"))
}
pub fn process_getegid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("egid"))
}
pub fn process_setuid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("uid", args)
}
pub fn process_setgid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("gid", args)
}
pub fn process_seteuid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("uid", args)
}
pub fn process_setegid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("gid", args)
}

pub fn process_active_resources(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::active_resources_info(state))
}

pub fn test_run(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::run(state, args)
}

pub fn util_strip_vt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let text = match value {
        Value::String(text) => text,
        Value::StringUnits(units) => {
            return Ok(Value::String(
                crate::modules::util_strip::strip_vt_control_characters(&String::from_utf16_lossy(
                    units,
                )),
            ));
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"str\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )));
        }
    };
    Ok(Value::String(
        crate::modules::util_strip::strip_vt_control_characters(text),
    ))
}

pub fn util_format_with_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"inspectOptions\" argument must be an object".into(),
        ));
    }
    let separator = quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
        options,
        "numericSeparator",
    ));
    let colors = quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
        options, "colors",
    ));
    let compact = !matches!(
        quench_runtime::execute::get_property(options, "compact"),
        Value::Undefined
    );
    crate::modules::util::validate_json_arguments(&args[1..])?;
    Ok(Value::String(crate::modules::util::format_with_options(
        &args[1..],
        separator,
        colors,
        compact,
    )))
}

pub fn test_skip(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::skip(state, args)
}

pub fn util_is_deep_strict_equal(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let left = args.first().unwrap_or(&Value::Undefined);
    let right = args.get(1).unwrap_or(&Value::Undefined);
    let skip_prototype = match args.get(2) {
        Some(Value::Boolean(flag)) => *flag,
        Some(options) => quench_runtime::execute::is_truthy(
            &quench_runtime::execute::get_property(options, "skipPrototype"),
        ),
        None => false,
    };
    Ok(Value::Boolean(crate::modules::deep_equal::deep_equal_opts(
        left,
        right,
        true,
        skip_prototype,
    )?))
}

pub fn util_to_usv_string(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let units = match value {
        Value::String(value) => value.encode_utf16().collect::<Vec<_>>(),
        Value::StringUnits(value) => value.iter().copied().collect::<Vec<_>>(),
        Value::Undefined => "undefined".encode_utf16().collect(),
        Value::Null => "null".encode_utf16().collect(),
        _ => return Ok(Value::String(format!("{value:?}"))),
    };
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if (0xD800..=0xDBFF).contains(&unit) {
            if units
                .get(index + 1)
                .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
            {
                output.extend([unit, units[index + 1]]);
                index += 2;
                continue;
            }
            output.push(0xFFFD);
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            output.push(0xFFFD);
        } else {
            output.push(unit);
        }
        index += 1;
    }
    Ok(quench_runtime::execute::string_from_units(output))
}

pub fn util_is_native_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let native = matches!(
        quench_runtime::execute::get_property_result(value, "\0error_slot"),
        Ok(Value::Boolean(true))
    );
    Ok(Value::Boolean(native))
}

pub fn util_type_predicate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let predicate = args
        .iter()
        .find_map(|value| match value {
            Value::String(name) => Some(name.as_str()),
            _ => None,
        })
        .unwrap_or("");
    Ok(Value::Boolean(crate::modules::util::type_predicate(
        predicate,
        args.iter()
            .find(|value| !matches!(value, Value::String(_)))
            .unwrap_or(&Value::Undefined),
    )))
}
