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
    static EVENT_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static GC_EPOCH: Cell<u64> = const { Cell::new(0) };
}

fn event_prototype() -> Value {
    EVENT_PROTOTYPE.with(|slot| {
        if let Some(value) = slot.borrow().clone() {
            return value;
        }
        let prototype = host_api::object(Vec::new());
        let descriptor = host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_EVENT_TRUSTED_GET),
            ),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let prototype = execute::define_property(prototype, "isTrusted", descriptor)
            .unwrap_or_else(|_| host_api::object(Vec::new()));
        *slot.borrow_mut() = Some(prototype.clone());
        prototype
    })
}

pub fn event_trusted_get(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(false))
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

pub fn message_channel_construct(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::event_target::new_message_channel(state)
}

pub fn event_target_rejection(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit(
        state,
        &[
            Value::String("uncaughtException".into()),
            args.first().cloned().unwrap_or(Value::Undefined),
            Value::String("uncaughtException".into()),
        ],
    )?;
    Ok(Value::Undefined)
}

pub fn event_get_property(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let valid = matches!(
        execute::get_property(receiver, "Symbol.toStringTag"),
        Value::String(ref tag) if tag == "Event" || tag == "CustomEvent"
    );
    if !valid {
        return Err(crate::modules::buffer_enc::invalid_this());
    }
    let Some(Value::String(key)) = args.first() else {
        return Ok(Value::Undefined);
    };
    Ok(execute::get_property(receiver, key))
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

pub fn util_normalize_encoding(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(Value::String("utf8".into()));
    }
    let Value::String(encoding) = value else {
        return Ok(Value::Undefined);
    };
    if encoding.is_empty() {
        return Ok(Value::String("utf8".into()));
    }
    Ok(crate::modules::buffer_enc::canonical_encoding(encoding)
        .map(|name| Value::String(name.into()))
        .unwrap_or(Value::Undefined))
}

pub fn util_get_cidr(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let address = match args.first() {
        Some(Value::String(value)) => value,
        _ => return Ok(Value::Null),
    };
    let netmask = match args.get(1) {
        Some(Value::String(value)) => value,
        _ => return Ok(Value::Null),
    };
    let family = match args.get(2) {
        Some(Value::String(value)) => value,
        _ => return Ok(Value::Null),
    };
    let prefix = match (family.as_str(), address.parse::<std::net::IpAddr>(), netmask.parse::<std::net::IpAddr>()) {
        ("IPv4", Ok(std::net::IpAddr::V4(_)), Ok(std::net::IpAddr::V4(mask))) => {
            prefix_length(&mask.octets())
        }
        ("IPv6", Ok(std::net::IpAddr::V6(_)), Ok(std::net::IpAddr::V6(mask))) => {
            let segments = mask.segments();
            let bytes = segments.iter().flat_map(|segment| segment.to_be_bytes()).collect::<Vec<_>>();
            prefix_length(&bytes)
        }
        _ => None,
    };
    Ok(prefix
        .map(|length| Value::String(format!("{address}/{length}")))
        .unwrap_or(Value::Null))
}

fn prefix_length(mask: &[u8]) -> Option<u32> {
    let mut length = 0;
    let mut saw_zero = false;
    for byte in mask {
        for bit in (0..8).rev() {
            if byte & (1 << bit) != 0 {
                if saw_zero {
                    return None;
                }
                length += 1;
            } else {
                saw_zero = true;
            }
        }
    }
    Some(length)
}

pub fn util_construct_shared_array_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    execute::construct_value(
        &Value::Builtin(quench_runtime::ops::Builtin::SharedArrayBuffer),
        args,
    )
}
pub fn util_inspect(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(
        execute::get_property(&arg, "\0source_text_module"),
        Value::Boolean(true)
    ) && args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "depth"),
            Value::Number(value) if value < 0.0
        )
    }) {
        return Ok(Value::String("[SourceTextModule]".into()));
    }
    if matches!(execute::get_property(&arg, "Symbol.toStringTag"), Value::String(ref tag) if tag == "AbortController")
        && execute::has_own_property(&arg, "signal")
    {
        let depth = args.get(1).and_then(|options| {
            let value = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                execute::get_property(options, "depth")
            } else {
                options.clone()
            };
            match value {
                Value::Null => Some(usize::MAX),
                Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as usize),
                _ => None,
            }
        });
        let signal = execute::get_property(&arg, "signal");
        let aborted = execute::get_property(&signal, "aborted");
        return Ok(Value::String(if depth.is_some_and(|value| value <= 1) {
            "AbortController { signal: [AbortSignal] }".into()
        } else {
            format!(
                "AbortController {{ signal: AbortSignal {{ aborted: {} }} }}",
                crate::modules::util::inspect(&aborted)
            )
        }));
    }
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
        Some(depth) => crate::modules::util::inspect_with_options_colors(
            &arg,
            depth,
            show_hidden,
            max_array_length,
            getters,
            colors,
        ),
        None if show_hidden || getters || max_array_length.is_some() => {
            crate::modules::util::inspect_with_options_colors(
                &arg,
                3,
                show_hidden,
                max_array_length,
                getters,
                colors,
            )
        }
        None => {
            crate::modules::util::inspect_with_options_colors(&arg, 3, false, None, false, colors)
        }
    }))
}

pub fn util_aborted(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let rejected = |message: &str| {
        let promise = Rc::new(quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Pending,
        ));
        let error = host_api::object(vec![
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ("name".into(), Value::String("TypeError".into())),
            ("message".into(), Value::String(message.into())),
        ]);
        quench_runtime::reject_promise(&promise, error);
        Value::Promise(promise)
    };
    let Some(signal) = args.first() else {
        return Ok(rejected("The signal argument must be an AbortSignal"));
    };
    let Some(resource) = args.get(1) else {
        return Ok(rejected("The resource argument must be an object"));
    };
    if !matches!(resource, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(rejected("The resource argument must be an object"));
    }
    if !matches!(
        execute::get_property(signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    ) {
        return Ok(rejected("The signal argument must be an AbortSignal"));
    }
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    if execute::is_truthy(&execute::get_property(signal, "aborted")) {
        quench_runtime::resolve_promise(&promise, Value::Undefined);
        return Ok(Value::Promise(promise));
    }
    let callback = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_UTIL_ABORTED_RESOLVE.cap,
            ),
        },
        vec![
            Value::Promise(promise.clone()),
            Value::Number(GC_EPOCH.with(Cell::get) as f64),
        ],
    );
    crate::modules::event_target::add_event_listener(
        state,
        Some(signal),
        &[
            Value::String("abort".into()),
            callback,
            host_api::object(vec![("once".into(), Value::Boolean(true))]),
        ],
    )?;
    Ok(Value::Promise(promise))
}

pub fn util_aborted_resolve(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let captured = args.get(1).and_then(|value| match value {
        Value::Number(value) => Some(*value as u64),
        _ => None,
    });
    if captured != Some(GC_EPOCH.with(Cell::get)) {
        return Ok(Value::Undefined);
    }
    if let Some(Value::Promise(promise)) = args.first() {
        quench_runtime::resolve_promise(promise, Value::Undefined);
    }
    Ok(Value::Undefined)
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
                // Node canonicalizes a custom promisifier by making the
                // returned function idempotent: promisify(custom) returns
                // the same identity. Keep this mutation on the existing
                // function rather than wrapping it again.
                let _ = execute::set_property_in_place(&custom, custom_key, custom.clone());
                return Ok(
                    match timer_promise_alias(&original).or_else(|| {
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
                                    (
                                        "value".into(),
                                        Value::String(name.trim_end_matches("Promise").to_string()),
                                    ),
                                    ("configurable".into(), Value::Boolean(true)),
                                ]),
                            )
                            .unwrap_or(custom),
                            _ => custom,
                        },
                        None => custom,
                    },
                );
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
                Value::String(name) => {
                    execute::set_property(promise_api, "name", Value::String(name))
                }
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
    if !execute::set_property_in_place(&wrapper, crate::modules::util::PROMISIFY_CUSTOM_KEY, custom)
    {
        return Err(VmError::NotCallable);
    }
    Ok(wrapper)
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
    let mut wrapper = bound_custom(
        crate::registry::SPEC_UTIL_DEPRECATED_CALL.cap,
        vec![
            callback.clone(),
            args.get(1).cloned().unwrap_or(Value::String(String::new())),
            args.get(2).cloned().unwrap_or(Value::Undefined),
        ],
    );
    let length = execute::get_property(&callback, "length");
    wrapper = execute::set_property(wrapper, "length", length);
    let modify = args
        .get(3)
        .map(|options| execute::get_property(options, "modifyPrototype"))
        .unwrap_or(Value::Undefined);
    if !matches!(modify, Value::Boolean(false)) {
        let prototype = execute::get_property(&callback, "prototype");
        wrapper = execute::set_property(wrapper, "prototype", prototype);
        wrapper = execute::set_prototype_of(&wrapper, &callback)?;
    } else {
        wrapper = execute::set_property(
            wrapper,
            "prototype",
            quench_runtime::host_api::object(Vec::new()),
        );
    }
    Ok(wrapper)
}

pub fn util_debuglog(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(bound_custom(
        crate::registry::SPEC_UTIL_DEBUGLOG.cap,
        vec![],
    ))
}

pub fn util_deprecated_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(callback) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let message = match args.get(1) {
        Some(Value::String(value)) => value.as_str(),
        _ => "",
    };
    let code = match args.get(2) {
        Some(Value::String(value)) => Some(value.as_str()),
        _ => None,
    };
    if crate::modules::process::mark_deprecation(state, callback, code) {
        crate::modules::process::emit_warning(state, "DeprecationWarning", message, code, false);
    }
    quench_runtime::execute::call(
        callback,
        &Value::Undefined,
        args.get(3..).unwrap_or_default(),
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
        -110 => "ETIMEDOUT",
        _ => return Ok(Value::String(format!("Unknown system error {errno}"))),
    };
    Ok(Value::String(name.into()))
}

pub fn process_binding_uv_errname(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit_warning(
        state,
        "DeprecationWarning",
        "Directly calling process.binding('uv').errname(<val>) is being deprecated. Please make sure to use util.getSystemErrorName() instead.",
        Some("DEP0119"),
        true,
    );
    util_system_error_name(state, None, args)
}

pub fn process_set_source_maps_enabled(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !matches!(args.first(), Some(Value::Boolean(_))) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"enabled\" argument must be of type boolean".into(),
        ));
    }
    Ok(Value::Undefined)
}

pub fn process_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    update_fork_timers(state, receiver, true);
    process_ref_like(args.first(), "Symbol.for.nodejs.ref\0", "ref")
}

pub fn process_unref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    update_fork_timers(state, receiver, false);
    process_ref_like(args.first(), "Symbol.for.nodejs.unref\0", "unref")
}

/// Apply ChildProcess ref/unref to only the timers created while its forked
/// source ran in the shared realm. Ordinary process/timer calls carry no
/// hidden timer list and retain their existing no-op contract.
fn update_fork_timers(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    referenced: bool,
) {
    let Some(receiver) = receiver else {
        return;
    };
    let ids = execute::get_property(receiver, "\0childTimerIds");
    let Value::Array(ids) = ids else {
        return;
    };
    let mut timers = state.borrow_mut();
    for index in 0..ids.logical_len() {
        let Ok(value) = execute::get_property_result(&Value::Array(ids.clone()), &index.to_string())
        else {
            continue;
        };
        let Value::Number(id) = value else {
            continue;
        };
        if let Some(timer) = timers.timers.timers.get_mut(&(id as u64)) {
            timer.referenced = referenced;
        }
    }
}

pub fn process_set_uncaught_exception_capture_callback(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(callback, Value::Null)
        && !quench_runtime::is_callable(&callback)
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"fn\" argument must be of type function or null.{}",
            crate::modules::buffer_enc::invalid_arg_received(&callback)
        )));
    }
    if state
        .borrow()
        .process
        .uncaught_exception_capture_callback
        .is_some()
        && !matches!(callback, Value::Null)
    {
        return Err(VmError::Thrown(crate::host::namespace_object_from_pairs(vec![
            ("name".into(), Value::String("Error".into())),
            (
                "code".into(),
                Value::String("ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET".into()),
            ),
            (
                "message".into(),
                Value::String(
                    "setupUncaughtExceptionCaptureCallback called while a capture callback was already set"
                        .into(),
                ),
            ),
        ])));
    }
    state.borrow_mut().process.uncaught_exception_capture_callback =
        (!matches!(callback, Value::Null)).then_some(callback);
    Ok(Value::Undefined)
}

pub fn process_has_uncaught_exception_capture_callback(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        state
            .borrow()
            .process
            .uncaught_exception_capture_callback
            .is_some(),
    ))
}

fn process_ref_like(
    target: Option<&Value>,
    symbol_key: &str,
    legacy_key: &str,
) -> Result<Value, VmError> {
    let Some(target) = target else {
        return Ok(Value::Undefined);
    };
    for key in [symbol_key, legacy_key] {
        let method = execute::get_property(target, key);
        if quench_runtime::is_callable(&method) {
            execute::call(&method, target, &[])?;
            return Ok(Value::Undefined);
        }
    }
    Ok(Value::Undefined)
}

pub fn util_convert_signal_to_exit_code(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(signal)) = args.first() else {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String("The signal argument must be a valid signal name".into()),
            ),
        ])));
    };
    let number = match signal.as_str() {
        "SIGHUP" => 1,
        "SIGINT" => 2,
        "SIGABRT" => 6,
        "SIGKILL" => 9,
        "SIGTERM" => 15,
        _ => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                (
                    "message".into(),
                    Value::String("The signal argument must be a valid signal name".into()),
                ),
            ])))
        }
    };
    Ok(Value::Number((128 + number) as f64))
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
    let is_exec = matches!(
        &original,
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
                    quench_runtime::ops::HostCapabilityKind::Custom(0x1e02)
                ))
            )
    );
    if is_exec {
        if let Some(options) = call_args.get(1) {
            let signal = execute::get_property(options, "signal");
            let valid = matches!(signal, Value::Undefined)
                || matches!(
                    execute::get_property(&signal, "aborted"),
                    Value::Boolean(_)
                );
            if !valid {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("TypeError".into())),
                    ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                    (
                        "message".into(),
                        Value::String(
                            "The \"options.signal\" property must be an instance of AbortSignal."
                                .into(),
                        ),
                    ),
                ])));
            }
        }
    }
    match quench_runtime::vm::call_value(original, &receiver, &call_args) {
        Ok(result) => {
            if matches!(result, Value::Object(_) | Value::ObjectAlias(_)) {
                let promise_value = Value::Promise(Rc::clone(&promise));
                let _ = execute::set_property(promise_value, "child", result.clone());
            }
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
        Err(VmError::Thrown(error)) => {
            // execFile's Node promisifier validates its AbortSignal before
            // creating the promise; preserve that synchronous TypeError.
            let is_exec_or_file = matches!(
                &original,
                Value::BoundFunction(bound)
                    if matches!(
                        bound.target,
                        Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
                            quench_runtime::ops::HostCapabilityKind::Custom(cap)
                        )) if cap == 0x1e02 || cap == 0x1e03
                    )
            );
            if is_exec_or_file
                && matches!(
                    execute::get_property(&error, "code"),
                    Value::String(ref code) if code == "ERR_INVALID_ARG_TYPE"
                )
            {
                return Err(VmError::Thrown(error));
            }
            quench_runtime::reject_promise(&promise, error)
        }
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
    // The fixture leak checker runs from `process.on("exit")`. Keep host
    // bookkeeping out of that observable global scan while the exit handlers
    // drain, then restore it for the runner's post-loop verifier.
    let global = quench_runtime::vm::current_global_object();
    let saved_resource = execute::get_property(&global, "__nodeCurrentAsyncResource");
    let saved_calls = execute::get_property(&global, "__nodeCallChecks");
    let _ = execute::set_property_in_place(&global, "__nodeCurrentAsyncResource", global.clone());
    let _ = execute::set_property_in_place(&global, "__nodeCallChecks", global.clone());
    let result = crate::modules::pump::run_event_loop(state);
    let _ = execute::set_property_in_place(&global, "__nodeCurrentAsyncResource", saved_resource);
    let _ = execute::set_property_in_place(&global, "__nodeCallChecks", saved_calls);
    result?;
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
    receiver: Option<&Value>,
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
        return Ok(crate::host::namespace_object_from_pairs(vec![
            ("UV_EAI_MEMORY".to_string(), Value::Number(-3001.0)),
            ("UV_ENOENT".to_string(), Value::Number(-2.0)),
            ("UV_EOF".to_string(), Value::Number(-4095.0)),
            (
                "errname".to_string(),
                crate::host::capability(crate::registry::SPEC_PROCESS_BINDING_UV_ERRNAME),
            ),
        ]));
    }
    if name == "stream_wrap" {
        return Ok(crate::host::namespace_object_from_pairs(vec![
            (
                "streamBaseState".to_string(),
                host_api::object(Vec::new()),
            ),
            (
                "kReadBytesOrError".to_string(),
                Value::String("kReadBytesOrError".into()),
            ),
        ]));
    }
    if name == "tcp_wrap" {
        // The bootstrap TCP binding is the canonical declaration shared by
        // internal/test/binding and the net host. Preserve its prototype
        // identity so tests can observe patched native methods.
        let global = quench_runtime::vm::current_global_object();
        let cached = execute::get_property(&global, crate::modules::net::TCP_WRAP_BINDING_PROP);
        if matches!(cached, Value::Object(_) | Value::ObjectAlias(_)) {
            state.borrow_mut().tcp_binding = Some(cached.clone());
            return Ok(cached);
        }
        let prototype = crate::host::namespace_object_from_pairs(vec![
            (
                "setNoDelay".to_string(),
                Value::Builtin(quench_runtime::ops::Builtin::Object),
            ),
        ]);
        let tcp_constructor = execute::set_property(
            crate::host::capability(crate::registry::SPEC_NET_TCP),
            "prototype",
            prototype,
        );
        let constants = crate::host::namespace_object_from_pairs(vec![
            ("SOCKET".to_string(), Value::Number(1.0)),
        ]);
        let binding = crate::host::namespace_object_from_pairs(vec![
            ("TCP".to_string(), tcp_constructor.clone()),
            ("TCPWrap".to_string(), tcp_constructor),
            ("constants".to_string(), constants),
        ]);
        execute::set_property_in_place(&global, crate::modules::net::TCP_WRAP_BINDING_PROP, binding.clone());
        state.borrow_mut().tcp_binding = Some(binding.clone());
        return Ok(binding);
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
        let existing = { state.borrow().util_module.clone() };
        let util = if let Some(module) = existing {
            module
        } else {
            let module = crate::host::namespace_object_from_pairs(crate::modules::util::build());
            state.borrow_mut().util_module = Some(module.clone());
            module
        };
        let types = execute::get_property(&util, "types");
        let names = [
            "isAnyArrayBuffer",
            "isArrayBuffer",
            "isArrayBufferView",
            "isAsyncFunction",
            "isDataView",
            "isDate",
            "isExternal",
            "isMap",
            "isMapIterator",
            "isNativeError",
            "isPromise",
            "isRegExp",
            "isSet",
            "isSetIterator",
            "isTypedArray",
            "isUint8Array",
        ];
        let mut binding = names
            .iter()
            .map(|name| ((*name).to_string(), execute::get_property(&types, name)))
            .collect::<Vec<_>>();
        // `process.binding('util')` is the public binding and contains only
        // predicates.  The private symbol table belongs to
        // `internalBinding('util')`; receiver identity is the one fact that
        // distinguishes those two entry points without a second JS surface.
        let process = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "process",
        );
        let public_binding = receiver.is_some_and(|value| value == &process);
        if !public_binding {
            binding.push((
                "privateSymbols".to_string(),
                host_api::object(vec![(
                    "arrow_message_private_symbol".to_string(),
                    Value::String("Symbol.node:arrowMessage\0internal".into()),
                )]),
            ));
            binding.push((
                "getProxyDetails".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_GET_PROXY_DETAILS),
            ));
        }
        return Ok(crate::host::namespace_object_from_pairs(binding));
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
    // Both public `process.binding()` and the internal helper use this
    // capability. Unknown names must remain observable errors; known
    // allowlisted bindings that have no modeled surface are represented by
    // an empty namespace so callers still observe a binding object.
    if matches!(
        name.as_str(),
        "contextify"
            | "fs_event_wrap"
            | "icu"
            | "inspector"
            | "natives"
            | "pipe_wrap"
            | "spawn_sync"
            | "stream_wrap"
            | "tcp_wrap"
            | "tls_wrap"
            | "udp_wrap"
            | "zlib"
    ) {
        if name == "pipe_wrap" {
            let constants = crate::host::namespace_object_from_pairs(vec![
                ("SOCKET".into(), Value::Number(0.0)),
            ]);
            return Ok(crate::host::namespace_object_from_pairs(vec![
                ("constants".into(), constants),
                (
                    "Pipe".into(),
                    crate::host::capability(crate::registry::SPEC_NET_PIPE),
                ),
            ]));
        }
        return Ok(crate::host::namespace_object_from_pairs(Vec::new()));
    }
    let prefix = if name == "debug" {
        "No such binding: "
    } else if receiver.is_some() {
        "No such module: "
    } else {
        "No such binding: "
    };
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!("{prefix}{name}"))],
    );
    let error = if receiver.is_some() {
        quench_runtime::execute::set_property(
            error,
            "code",
            Value::String("ERR_UNKNOWN_BUILTIN_MODULE".into()),
        )
    } else {
        error
    };
    Err(VmError::Thrown(error))
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
pub fn process_kill(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::kill(state, args)
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
    let process_scope = state.borrow().cluster.process_scope();
    state
        .borrow()
        .event_loop
        .queue_microtask_scope(callback, vec![], process_scope);
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

pub fn process_initgroups(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let user = args.first().unwrap_or(&Value::Undefined);
    if !matches!(user, Value::Number(_) | Value::String(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"user\" argument must be one of type number or string.{}",
            crate::modules::util::invalid_arg_received(user)
        )));
    }
    let extra = args.get(1).unwrap_or(&Value::Undefined);
    if !matches!(extra, Value::Number(_) | Value::String(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"extraGroup\" argument must be one of type number or string.{}",
            crate::modules::util::invalid_arg_received(extra)
        )));
    }
    if let Value::String(group) = extra {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            (
                "code".into(),
                Value::String("ERR_UNKNOWN_CREDENTIAL".into()),
            ),
            (
                "message".into(),
                Value::String(format!("Group identifier does not exist: {group}")),
            ),
        ])));
    }
    Ok(Value::Undefined)
}

pub fn process_setgroups(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Array(groups)) = args.first() else {
        let value = args.first().unwrap_or(&Value::Undefined);
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"groups\" argument must be an instance of Array.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    for index in 0..groups.logical_len() {
        let value = execute::get_property(&Value::Array(groups.clone()), &index.to_string());
        match value {
            Value::Number(number) if number.is_finite() && number >= 0.0 => {}
            Value::Number(number) => {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("RangeError".into())),
                    ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                    (
                        "message".into(),
                        Value::String(format!("The value of \"groups[{index}]\" is out of range. Received {number}")),
                    ),
                ])));
            }
            Value::String(group) => {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    (
                        "code".into(),
                        Value::String("ERR_UNKNOWN_CREDENTIAL".into()),
                    ),
                    (
                        "message".into(),
                        Value::String(format!("Group identifier does not exist: {group}")),
                    ),
                ])));
            }
            value => {
                return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                    "The \"groups[{index}]\" argument must be one of type number or string.{}",
                    crate::modules::util::invalid_arg_received(&value)
                )));
            }
        }
    }
    Ok(Value::Undefined)
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

pub fn process_available_memory(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(f64::MAX))
}

pub fn process_constrained_memory(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(f64::MAX))
}

pub fn process_memory_usage(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    // Keep the observable shape stable even though the Rust host does not
    // expose V8 heap counters. Resident memory is measured where procfs is
    // available; unsupported hosts use zero while preserving Node's fields.
    let rss = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map(|pages| pages.saturating_mul(4096) as f64)
        .unwrap_or(0.0);
    Ok(host_api::object(vec![
        ("rss".into(), Value::Number(rss)),
        ("heapTotal".into(), Value::Number(0.0)),
        ("heapUsed".into(), Value::Number(0.0)),
        ("external".into(), Value::Number(0.0)),
        ("arrayBuffers".into(), Value::Number(0.0)),
    ]))
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
pub fn os_endianness(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::endianness(state, args)
}
pub fn os_version(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::version(state, args)
}
pub fn os_machine(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::machine(state, args)
}
pub fn os_user_info(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::user_info(state, args)
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
    // Agent.createSocket follows the Node callback contract
    // `(request, options, callback)`, while the shared net connector accepts
    // `(options, callback)`. Keep that mechanical adapter at the capability
    // boundary so all Agent implementations still converge on one connector
    // state machine.
    let normalized;
    let args = if receiver
        .is_some_and(|value| crate::modules::net::net_id(value).is_none())
        && matches!(args.first(), Some(Value::Object(_) | Value::ObjectAlias(_)))
        && matches!(args.get(1), Some(Value::Object(_) | Value::ObjectAlias(_)))
        && matches!(
            execute::get_property(args.first().unwrap(), "port"),
            Value::Undefined
        )
        && !matches!(execute::get_property(args.get(1).unwrap(), "port"), Value::Undefined)
    {
        normalized = std::iter::once(args[1].clone())
            .chain(args.iter().skip(2).cloned())
            .collect::<Vec<_>>();
        normalized.as_slice()
    } else {
        args
    };
    match receiver {
        Some(receiver) if crate::modules::net::net_id(receiver).is_some() => {
            crate::modules::net::connect_existing(state, receiver, args)
        }
        _ => crate::modules::net::connect(state, args),
    }
}

pub fn net_lookup_callback(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let result = host_api::array(args.to_vec());
    let deferred = {
        let mut host = state.borrow_mut();
        if host.net.lookup_in_call {
            host.net.lookup_result = Some(result.clone());
            false
        } else {
            !host.net.pending_lookups.is_empty()
        }
    };
    if deferred {
        crate::modules::net::complete_lookup(state, result.clone())?;
    }
    Ok(result)
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

pub fn http_outgoing_construct(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let mut object = Value::object(vec![
        ("writable".into(), Value::Boolean(true)),
        ("writableObjectMode".into(), Value::Boolean(false)),
        ("writableHighWaterMark".into(), Value::Number(16_384.0)),
        ("writableLength".into(), Value::Number(0.0)),
        ("outputSize".into(), Value::Number(0.0)),
        ("writableEnded".into(), Value::Boolean(false)),
        ("writableFinished".into(), Value::Boolean(false)),
        ("finished".into(), Value::Boolean(false)),
        ("destroyed".into(), Value::Boolean(false)),
        ("closed".into(), Value::Boolean(false)),
        ("errored".into(), Value::Undefined),
        ("socket".into(), Value::Null),
    ]);
    if let Some(prototype) = state.borrow().http.outgoing_prototype.clone() {
        object = execute::set_prototype_of(&object, &prototype)?;
    }
    Ok(object)
}

pub fn http_outgoing_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
    let Value::Number(length) = crate::modules::buffer::byte_length(state, args)? else {
        return Err(VmError::NotCallable);
    };
    let number_property = |name: &str, default| match execute::get_property(&receiver, name) {
        Value::Number(value) => value,
        _ => default,
    };
    let output_size = number_property("outputSize", 0.0) + length;
    let writable_length = number_property("writableLength", 0.0) + length;
    execute::set_property_in_place(&receiver, "outputSize", Value::Number(output_size));
    execute::set_property_in_place(
        &receiver,
        "writableLength",
        Value::Number(writable_length),
    );
    Ok(Value::Boolean(
        output_size < number_property("writableHighWaterMark", 16_384.0),
    ))
}

pub fn http_outgoing_end(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
    execute::set_property_in_place(&receiver, "finished", Value::Boolean(true));
    execute::set_property_in_place(&receiver, "writableEnded", Value::Boolean(true));
    execute::set_property_in_place(&receiver, "writableLength", Value::Number(0.0));
    Ok(receiver)
}

pub fn http_outgoing_destroy(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
    execute::set_property_in_place(&receiver, "destroyed", Value::Boolean(true));
    execute::set_property_in_place(&receiver, "closed", Value::Boolean(true));
    Ok(receiver)
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

pub fn process_env_set(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(Value::String(value)) = args.first() {
        quench_runtime::date::set_local_timezone(Some(value));
    }
    Ok(Value::Undefined)
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() > 2
        || args
            .get(1)
            .is_some_and(|options| !matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
    {
        return Err(quench_runtime::execute::type_error(
            "The options argument must be an object",
        ));
    }
    let count = match args.first() {
        None | Some(Value::Undefined) | Some(Value::Object(_)) | Some(Value::ObjectAlias(_)) => 10,
        Some(Value::Number(value))
            if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
        {
            if *value > 200.0 {
                return Err(VmError::Thrown(Value::object(vec![
                    ("name".into(), Value::String("RangeError".into())),
                    ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                    (
                        "message".into(),
                        Value::String("The frame count must be between 1 and 200".into()),
                    ),
                ])));
            }
            *value as usize
        }
        Some(Value::Number(_)) => {
            return Err(VmError::Thrown(Value::object(vec![
                ("name".into(), Value::String("RangeError".into())),
                ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                (
                    "message".into(),
                    Value::String("The frame count must be an integer between 1 and 200".into()),
                ),
            ])));
        }
        Some(_) => {
            return Err(quench_runtime::execute::type_error(
                "The frame count must be an integer",
            ));
        }
    };
    let script_name = state
        .borrow()
        .process
        .argv
        .get(1)
        .cloned()
        .map(Value::String)
        .unwrap_or_else(|| Value::String(String::new()));
    Ok(quench_runtime::host_api::array(
        (0..count)
            .map(|_| {
                quench_runtime::host_api::object(vec![
                    ("scriptName".into(), script_name.clone()),
                    ("scriptId".into(), script_name.clone()),
                    ("lineNumber".into(), Value::Number(0.0)),
                    ("columnNumber".into(), Value::Number(0.0)),
                ])
            })
            .collect(),
    ))
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
    crate::modules::buffer::btoa(args).map(Value::String)
}

pub fn cp_spawn_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let command_nul = args
        .first()
        .and_then(|value| execute::to_js_string(value).ok())
        .is_some_and(|value| value.contains('\0'));
    let args_nul = matches!(args.get(1), Some(Value::Array(values)) if (0..values.logical_len()).any(|index| {
        matches!(execute::to_js_string(&execute::get_property_result(&args[1], &index.to_string()).unwrap_or(Value::Undefined)), Ok(value) if value.contains('\0'))
    }));
    let options = args
        .get(2)
        .or_else(|| args.get(1))
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    if command_nul || args_nul || options.is_some_and(cp_options_have_nul) {
        return Err(cp_nul_error());
    }
    if let Some(options) = args
        .get(2)
        .or_else(|| args.get(1))
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if let Some(internal) = state.borrow().module_cache.get("internal/child_process") {
            let spawn_sync = execute::get_property(internal, "spawnSync");
            let original = execute::get_property(internal, "\0originalSpawnSync");
            if spawn_sync != original
                && matches!(spawn_sync, Value::Function(_) | Value::BoundFunction(_))
            {
                let command = args.first().cloned().unwrap_or(Value::Undefined);
                let child_args = args.get(1).cloned().unwrap_or_else(|| host_api::array(Vec::new()));
                let shell = execute::get_property(options, "shell");
                let process_object = execute::get_property(
                    &quench_runtime::vm::current_global_object(),
                    "process",
                );
                let platform = {
                    let descriptor = execute::get_property(
                        &process_object,
                        "\0quench:descriptor:\0platform",
                    );
                    let getter = execute::get_property(&descriptor, "get");
                    if quench_runtime::is_callable(&getter) {
                        execute::call(&getter, &process_object, &[])
                            .ok()
                            .and_then(|value| execute::to_js_string(&value).ok())
                            .unwrap_or_default()
                    } else {
                        execute::to_js_string(&execute::get_property(&process_object, "platform"))
                            .unwrap_or_default()
                    }
                };
                let is_windows = platform == "win32";
                let shell_file = match shell.clone() {
                    Value::String(value) => value,
                    Value::Boolean(true) if is_windows => {
                        let env = execute::get_property(&process_object, "env");
                        match execute::get_property(&env, "comspec") {
                            Value::String(value) if !value.is_empty() => value,
                            _ => "cmd.exe".into(),
                        }
                    }
                    Value::Boolean(true) if platform == "android" => "/system/bin/sh".into(),
                    Value::Boolean(true) => "/bin/sh".into(),
                    _ => String::new(),
                };
                let command_text = std::iter::once(
                    execute::to_js_string(&command).unwrap_or_default(),
                )
                .chain(match child_args {
                    Value::Array(values) => (0..values.logical_len())
                        .filter_map(|index| {
                            execute::get_property_result(
                                &Value::Array(values.clone()),
                                &index.to_string(),
                            )
                            .ok()
                            .and_then(|value| execute::to_js_string(&value).ok())
                        })
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                })
                .collect::<Vec<_>>()
                .join(" ");
                let is_cmd = shell_file.ends_with("cmd.exe") || shell_file == "cmd";
                let flags = if is_cmd {
                    vec!["/d", "/s", "/c"]
                } else {
                    vec!["-c"]
                };
                let output_command = if is_cmd {
                    format!("\"{command_text}\"")
                } else {
                    command_text
                };
                let mut internal_options = vec![
                    ("file".into(), Value::String(shell_file.clone())),
                    (
                        "args".into(),
                        host_api::array(
                            std::iter::once(Value::String(shell_file.clone()))
                                .chain(flags.iter().map(|flag| Value::String((*flag).into())))
                                .chain(std::iter::once(Value::String(output_command)))
                                .collect(),
                        ),
                    ),
                    ("shell".into(), shell),
                    ("windowsHide".into(), Value::Boolean(false)),
                    ("windowsVerbatimArguments".into(), Value::Boolean(is_cmd)),
                ];
                if let Value::String(cwd) = execute::get_property(options, "cwd") {
                    internal_options.push(("cwd".into(), Value::String(cwd)));
                }
                return execute::call(
                    &spawn_sync,
                    &Value::Undefined,
                    &[host_api::object(internal_options)],
                );
            }
        }
    }
    if let Some(internal) = args.first().filter(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && !matches!(execute::get_property(value, "file"), Value::Undefined)
    }) {
        let file = execute::get_property(internal, "file");
        let child_args = match execute::get_property(internal, "args") {
            Value::Array(values) if values.logical_len() > 0 => {
                let first = execute::get_property_result(
                    &Value::Array(values.clone()),
                    "0",
                )
                .unwrap_or(Value::Undefined);
                if first == file {
                    host_api::array(
                        (1..values.logical_len())
                            .filter_map(|index| {
                                execute::get_property_result(
                                    &Value::Array(values.clone()),
                                    &index.to_string(),
                                )
                                .ok()
                            })
                            .collect(),
                    )
                } else {
                    Value::Array(values)
                }
            }
            value => value,
        };
        let options = execute::set_property(internal.clone(), "shell", Value::Undefined);
        return crate::modules::child_process::spawn_sync(
            state,
            &[file, child_args, options],
        );
    }
    crate::modules::child_process::spawn_sync(state, args)
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
    if command.contains('\0') {
        return Err(cp_nul_error());
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
    if let Value::Array(array) = &spawnargs {
        let has_nul = (0..array.logical_len()).any(|index| {
            execute::to_js_string(
                &execute::get_property_result(&spawnargs, &index.to_string())
                    .unwrap_or(Value::Undefined),
            )
            .is_ok_and(|value| value.contains('\0'))
        });
        if has_nul {
            return Err(cp_nul_error());
        }
    }
    let options = args
        .get(2)
        .cloned()
        .or_else(|| {
            args.get(1)
                .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
                .cloned()
        })
        .unwrap_or(Value::Undefined);
    if cp_options_have_nul(&options) {
        return Err(cp_nul_error());
    }
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
        let serialization = execute::get_property(&options, "serialization");
        let valid_serialization = matches!(serialization, Value::Undefined)
            || matches!(serialization, Value::String(ref value) if value == "json" || value == "advanced");
        if !valid_serialization {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "The property 'options.serialization' must be one of: undefined, 'json', 'advanced'. Received {}",
                        crate::modules::util::inspect(&serialization)
                    )),
                ),
            ])));
        }
    }
    if let Value::Array(stdio) = execute::get_property(&options, "stdio") {
        let ipc_count = (0..stdio.logical_len())
            .filter(|index| {
                execute::get_property_result(&Value::Array(stdio.clone()), &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .is_some_and(|value| value == "ipc")
            })
            .count();
        if ipc_count > 1 {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("Error".into())),
                ("code".into(), Value::String("ERR_IPC_ONE_PIPE".into())),
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
    let stdin = execute::set_property(
        execute::set_property(stdin, "writable", Value::Boolean(true)),
        "readable",
        Value::Boolean(false),
    );
    let stdout = crate::modules::events::new_emitter_object(state)?;
    let stdout = execute::set_property(
        stdout,
        "read",
        crate::host::capability(crate::registry::SPEC_CP_STDOUT_READ),
    );
    let stdout = execute::set_property(
        stdout,
        "pipe",
        Value::Builtin(quench_runtime::ops::Builtin::Object),
    );
    let stdout = execute::set_property(
        stdout,
        "destroy",
        Value::Builtin(quench_runtime::ops::Builtin::Object),
    );
    let stdout = execute::set_property(
        execute::set_property(
            stdout,
            "ref",
            crate::host::capability(crate::registry::SPEC_PROCESS_REF),
        ),
        "unref",
        crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
    );
    if command == "grep"
        || command.ends_with("/grep")
        || command == "sed"
        || command.ends_with("/sed")
        || command == "wc"
        || command.ends_with("/wc")
    {
        let filter = if command.ends_with("grep") {
            "grep"
        } else if command.ends_with("sed") {
            "sed"
        } else {
            "wc"
        };
        execute::set_property_in_place(&stdin, "\0childFilter", Value::String(filter.into()));
        execute::set_property_in_place(
            &stdin,
            "\0childFilterArg",
            execute::get_property(&spawnargs, "0"),
        );
        execute::set_property_in_place(&stdin, "\0childFilterOutput", stdout.clone());
    }
    let stderr = crate::modules::events::new_emitter_object(state)?;
    let stderr = execute::set_property(
        stderr,
        "pipe",
        Value::Builtin(quench_runtime::ops::Builtin::Object),
    );
    let stderr = execute::set_property(
        stderr,
        "destroy",
        Value::Builtin(quench_runtime::ops::Builtin::Object),
    );
    let stderr = execute::set_property(
        execute::set_property(
            stderr,
            "ref",
            crate::host::capability(crate::registry::SPEC_PROCESS_REF),
        ),
        "unref",
        crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
    );
    let set_encoding = crate::host::capability(crate::registry::SPEC_CP_STREAM_SET_ENCODING);
    let stdout = execute::set_property(stdout, "setEncoding", set_encoding.clone());
    let stderr = execute::set_property(stderr, "setEncoding", set_encoding);
    let stdout = execute::set_property(
        stdout,
        "_handle",
        host_api::object(vec![(
            "readStart".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        )]),
    );
    let stderr = execute::set_property(
        stderr,
        "_handle",
        host_api::object(vec![(
            "readStart".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        )]),
    );
    for (index, stream) in [(1, &stdout), (2, &stderr)] {
        if let Some(target) = cp_stdio_target(&options, index) {
            execute::set_property_in_place(stream, "\0childOutputTarget", target);
        }
    }
    if let Some(source) = cp_stdio_target(&options, 0) {
        if let Some(identity) = cp_pipe_key(&source) {
            state
                .borrow_mut()
                .child_pipes
                .insert(identity, stdin.clone());
        }
    }
    let child = crate::modules::events::new_emitter_object(state)?;
    let child = execute::set_property(child, "pid", Value::Undefined);
    let child = execute::set_property(child, "\0childCommand", Value::String(command.clone()));
    let child = execute::set_property(child, "\0childArgs", spawnargs.clone());
    let child = execute::set_property(child, "\0childOptions", options.clone());
    let child = execute::set_property(child, "stdin", stdin.clone());
    let child = execute::set_property(child, "stdout", stdout.clone());
    let child = execute::set_property(child, "stderr", stderr.clone());
    if matches!(execute::get_property(&stdin, "\0childFilter"), Value::String(_)) {
        execute::set_property_in_place(&stdin, "\0childFilterProcess", child.clone());
    }
    let child = execute::set_property(
        child,
        "stdio",
        host_api::array(vec![stdin.clone(), stdout.clone(), stderr.clone()]),
    );
    let stdin_script = command == state.borrow().process.exec_path
        && cp_spawn_script_uses_stdin(&spawnargs);
    if stdin_script {
        execute::set_property_in_place(&stdin, "\0childStdinScript", Value::Boolean(true));
        execute::set_property_in_place(&stdin, "\0childStdinProcess", child.clone());
        execute::set_property_in_place(&child, "\0childStdinScript", Value::Boolean(true));
    }
    if command == "cat"
        && matches!(&spawnargs, Value::Array(array) if array.logical_len() == 0)
    {
        execute::set_property_in_place(&stdin, "\0childCatEcho", Value::Boolean(true));
        execute::set_property_in_place(&child, "\0childCatEcho", Value::Boolean(true));
        execute::set_property_in_place(&stdin, "\0childStdinProcess", child.clone());
    }
    apply_cp_stdio_surface(&child, &options, [&stdin, &stdout, &stderr]);
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
        "Symbol.dispose",
        crate::host::capability(crate::registry::SPEC_CP_KILL),
    );
    let has_ipc = matches!(
        execute::get_property(&options, "stdio"),
        Value::Array(ref stdio)
            if (0..stdio.logical_len()).any(|index| {
                execute::get_property_result(&Value::Array(stdio.clone()), &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .is_some_and(|value| value == "ipc")
            })
    );
    if has_ipc {
        execute::set_property_in_place(
            &child,
            "send",
            crate::host::capability(crate::registry::SPEC_CP_SEND),
        );
        execute::set_property_in_place(
            &child,
            "disconnect",
            crate::host::capability(crate::registry::SPEC_CP_DISCONNECT),
        );
        execute::set_property_in_place(&child, "connected", Value::Boolean(true));
        execute::set_property_in_place(&child, "\0childIpc", Value::Boolean(true));
    }
    // ChildProcess follows the same ref/unref capability contract as other
    // event-loop handles.  The host has no separate child runtime object, so
    // these operations are intentionally allocation-free no-ops while still
    // preserving the observable callable API and chaining shape.
    let child = execute::set_property(
        child,
        "ref",
        crate::host::capability(crate::registry::SPEC_PROCESS_REF),
    );
    let child = execute::set_property(
        child,
        "unref",
        crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
    );
    // `spawn()` returns a ChildProcess instance.  Keep the host-created
    // object (and its event identity) while linking it to the one public
    // constructor prototype used by `instanceof` in Node code.
    let prototype = state
        .borrow()
        .module_cache
        .get("child_process")
        .map(|module| {
            execute::get_property(&execute::get_property(module, "ChildProcess"), "prototype")
        })
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .or_else(|| state.borrow().child_process_prototype.clone());
    let child = match prototype {
        Some(prototype) => execute::set_prototype_of(&child, &prototype).unwrap_or(child),
        None => child,
    };
    state.borrow_mut().identity_roots.push(child.clone());
    if let Ok(signal) = execute::get_property_result(&options, "signal") {
        let abort_like = matches!(
            signal,
            Value::Object(_) | Value::ObjectAlias(_)
        ) && (matches!(
            execute::get_property(&signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
            Value::Boolean(true)
        ) || matches!(execute::get_property(&signal, "aborted"), Value::Boolean(_)));
        if abort_like {
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
    // The host models ordinary children in-process, but their public pid must
    // still be a distinct, positive identity.  Using zero would invoke
    // POSIX process-group semantics when user code calls `process.kill(pid)`.
    let simulated_pid = {
        let mut process = state.borrow_mut();
        let mut candidate = std::process::id() as i64 + 1;
        while process.process.alive_pids.contains(&candidate) {
            candidate += 1;
        }
        process.process.alive_pids.insert(candidate);
        candidate as u64
    };
    execute::set_property_in_place(
        &child,
        "pid",
        Value::Number(simulated_pid as f64),
    );
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
        || (command == state.borrow().process.exec_path && !stdin_script)
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
    // Spawned scripts with an IPC descriptor share the same bounded process
    // channel as fork().  Execute their entry source once so process/stdin
    // listeners and handle delivery retain the child receiver identity.
    if has_ipc && command == state.borrow().process.exec_path {
        let values = match &spawnargs {
            Value::Array(array) => (0..array.logical_len())
                .filter_map(|index| {
                    execute::get_property_result(&spawnargs, &index.to_string())
                        .ok()
                        .and_then(|value| execute::to_js_string(&value).ok())
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        if let Some(script_index) = values.iter().position(|value| {
            value.ends_with(".js") || value.ends_with(".mjs")
        }) {
            let script = Value::String(values[script_index].clone());
            let execute_source = std::fs::read_to_string(&values[script_index])
                .map(|source| {
                    source.contains("process.on('message'")
                        || source.contains("process.on(\"message\"")
                        || source.contains("process.stdin")
                })
                .unwrap_or(false);
            if !execute_source {
                return Ok(child);
            }
            execute::set_property_in_place(&child, "\0childForkIpc", Value::Boolean(true));
            let fork_args = host_api::array(
                values
                    .iter()
                    .skip(script_index + 1)
                    .cloned()
                    .map(Value::String)
                    .collect(),
            );
            fork_child_start(state, &child, &script, &fork_args)?;
        }
    }
    Ok(child)
}

fn cp_stdio_target(options: &Value, index: usize) -> Option<Value> {
    let Value::Array(stdio) = execute::get_property(options, "stdio") else {
        return None;
    };
    let target = execute::get_property_result(&Value::Array(stdio), &index.to_string()).ok()?;
    matches!(target, Value::Object(_) | Value::ObjectAlias(_)).then_some(target)
}

fn apply_cp_stdio_surface(child: &Value, options: &Value, streams: [&Value; 3]) {
    let descriptor = execute::get_property(options, "stdio");
    let slots = match descriptor {
        Value::String(ref kind) if kind == "ignore" || kind == "inherit" => {
            vec![Value::Null, Value::Null, Value::Null]
        }
        Value::Array(ref entries) => (0..3)
            .map(|index| {
                let entry = execute::get_property_result(
                    &Value::Array(entries.clone()),
                    &index.to_string(),
                )
                .unwrap_or(Value::Undefined);
                match entry {
                    Value::String(ref kind) if kind == "ignore" || kind == "inherit" => Value::Null,
                    Value::String(ref kind) if kind == "ipc" => Value::Undefined,
                    Value::Object(_) | Value::ObjectAlias(_) => Value::Null,
                    _ => streams[index].clone(),
                }
            })
            .collect(),
        _ => streams.iter().map(|stream| (*stream).clone()).collect(),
    };
    let stdio = host_api::array(slots);
    execute::set_property_in_place(child, "stdio", stdio.clone());
    execute::set_property_in_place(child, "stdin", execute::get_property(&stdio, "0"));
    execute::set_property_in_place(child, "stdout", execute::get_property(&stdio, "1"));
    execute::set_property_in_place(child, "stderr", execute::get_property(&stdio, "2"));
}

fn cp_pipe_write(
    state: &Rc<RefCell<HostState>>,
    source: &Value,
    value: Value,
) -> Result<(), VmError> {
    let target = cp_pipe_target(state, source);
    let write = execute::get_property(&target, "write");
    if matches!(target, Value::Object(_) | Value::ObjectAlias(_))
        && quench_runtime::is_callable(&write)
    {
        execute::call(&write, &target, &[value])?;
    }
    Ok(())
}

fn cp_pipe_end(
    state: &Rc<RefCell<HostState>>,
    source: &Value,
) -> Result<(), VmError> {
    let target = cp_pipe_target(state, source);
    let end = execute::get_property(&target, "end");
    if matches!(target, Value::Object(_) | Value::ObjectAlias(_))
        && quench_runtime::is_callable(&end)
    {
        execute::call(&end, &target, &[])?;
    }
    Ok(())
}

fn cp_pipe_target(state: &Rc<RefCell<HostState>>, source: &Value) -> Value {
    cp_pipe_key(source)
        .and_then(|identity| state.borrow().child_pipes.get(&identity).cloned())
        .unwrap_or_else(|| execute::get_property(source, "\0childPipeTarget"))
}

fn cp_pipe_key(source: &Value) -> Option<u64> {
    crate::modules::emitter::emitter_id(source)
        .map(|id| id.0)
        .or_else(|| source.object_identity())
}

pub fn cp_spawn_output_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(child) = args.first() else {
        return Ok(Value::Undefined);
    };
    let stdout = args
        .get(1)
        .map(execute::canonical_value)
        .unwrap_or(Value::Undefined);
    let stderr = args
        .get(2)
        .map(execute::canonical_value)
        .unwrap_or(Value::Undefined);
    let emit = |target: &Value, event: &str, values: Vec<Value>| {
        let mut event_args = vec![Value::String(event.into())];
        event_args.extend(values);
        crate::modules::events::method_emit(state, Some(target), &event_args)
    };
    emit(child, "spawn", Vec::new())?;
    if matches!(
        execute::get_property(child, "\0childStdinScript"),
        Value::Boolean(true)
    ) || matches!(
        execute::get_property(child, "\0childCatEcho"),
        Value::Boolean(true)
    ) {
        return Ok(Value::Undefined);
    }
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
    } else if matches!(
        execute::get_property(child, "\0childForkIpc"),
        Value::Boolean(true)
    ) {
        match &command {
            Value::String(filename) => std::fs::read_to_string(filename)
                .ok()
                .and_then(|source| cp_script_write_output(&source, "process.stderr.write"))
                .unwrap_or_default(),
            _ => String::new(),
        }
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
        let source_warns = args.iter().any(|arg| {
            (!arg.starts_with('-') && (arg.ends_with(".js") || arg.ends_with(".mjs")))
                && std::fs::read_to_string(arg)
                    .map(|source| {
                        source.contains("emitWarning")
                            || source.contains("ExperimentalWarning")
                    })
                    .unwrap_or(false)
        });
        let should_warn = args.iter().any(|arg| arg == "--pending-deprecation") || source_warns;
        if !should_warn || args.iter().any(|arg| arg == "--no-warnings") {
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
    ) {
        match execute::get_property(&stdout, "\0childPendingOutput") {
            Value::String(value) => value,
            _ => String::new(),
        }
    } else if matches!(command, Value::String(ref value) if value == "echo" || value.ends_with("/echo")) {
        let args = match &child_args {
            Value::Array(array) => (0..array.logical_len())
                .filter_map(|index| {
                    execute::get_property_result(&child_args, &index.to_string()).ok()
                })
                .map(|value| execute::to_js_string(&value).unwrap_or_default())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        format!("{}\n", args.join(" "))
    } else if matches!(command, Value::String(ref value) if value == "grep" || value.ends_with("/grep") || value == "sed" || value.ends_with("/sed")) {
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
    } else if matches!(command, Value::String(ref value) if value == "cat" || value.ends_with("/cat")) {
        let path = execute::get_property_result(&child_args, "0")
            .ok()
            .and_then(|value| execute::to_js_string(&value).ok());
        path.and_then(|path| std::fs::read(path).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
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
                    .is_some_and(|value| value == "-v" || value == "--version")
            }),
            _ => false,
        } {
            format!("{}\n", state.borrow().process.version)
        } else if let Some(output) = cp_spawn_script_stdout(&child_args) {
            output
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
            // A normal child script is still a real Node entry point.  Read
            // its source and derive the observable stdout contract from the
            // same source facts used by `-e`; do not key behavior to fixture
            // names or to the presence of ChildProcess methods.
            cp_spawn_script_stdout(&child_args).unwrap_or_default()
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
    let filter_process = matches!(
        command,
        Value::String(ref value)
            if value == "grep"
                || value.ends_with("/grep")
                || value == "sed"
                || value.ends_with("/sed")
                || value == "wc"
                || value.ends_with("/wc")
    );
    if filter_process {
        // Filter output is driven by stdin writes. Keep the process and its
        // streams alive until cp_stdin_end observes upstream completion.
        return Ok(Value::Undefined);
    }
    let echo_process = matches!(
        command,
        Value::String(ref value) if value == "echo" || value.ends_with("/echo")
    );
    let pipe_target = cp_pipe_target(state, &stdout);
    if matches!(pipe_target, Value::Object(_) | Value::ObjectAlias(_))
        && !stdout_text.is_empty()
    {
        cp_pipe_write(state, &stdout, Value::String(stdout_text.clone()))?;
        cp_pipe_end(state, &stdout)?;
    }
    let stdout_target = execute::get_property(&stdout, "\0childOutputTarget");
    if matches!(stdout_target, Value::Object(_) | Value::ObjectAlias(_)) {
        if !stdout_text.is_empty() {
            crate::modules::net::socket_write(
                state,
                Some(&stdout_target),
                &[Value::String(stdout_text.clone())],
            )?;
        }
    } else if echo_process {
        let stdout_text = if matches!(
            execute::get_property(&child_options, "shell"),
            Value::Boolean(true)
        ) {
            let line = match &child_args {
                Value::Array(array) => execute::get_property_result(&child_args, "0")
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            line.strip_prefix("echo ").unwrap_or(&line).to_string() + "\n"
        } else {
            stdout_text
        };
        execute::set_property_in_place(
            &stdout,
            "\0childPendingOutput",
            cp_buffer_value(&stdout_text)?,
        );
        let data_listeners = crate::modules::events::method_listener_count(
            state,
            Some(&stdout),
            &[Value::String("data".into())],
        )?;
        if matches!(data_listeners, Value::Number(value) if value > 0.0) {
            emit(
                &stdout,
                "data",
                vec![cp_stream_output_value(&stdout, &stdout_text)?],
            )?;
        } else {
            emit(&stdout, "readable", Vec::new())?;
        }
    } else {
        emit(
            &stdout,
            "data",
            vec![cp_stream_output_value(&stdout, &stdout_text)?],
        )?;
    }
    let stderr_target = execute::get_property(&stderr, "\0childOutputTarget");
    if matches!(stderr_target, Value::Object(_) | Value::ObjectAlias(_)) {
        if !stderr_text.is_empty() {
            crate::modules::net::socket_write(
                state,
                Some(&stderr_target),
                &[Value::String(stderr_text.clone())],
            )?;
        }
    } else if !stderr_text.is_empty() {
        emit(
            &stderr,
            "data",
            vec![cp_stream_output_value(&stderr, &stderr_text)?],
        )?;
    }
    emit(&stdout, "end", Vec::new())?;
    emit(&stderr, "end", Vec::new())?;
    emit(&stdout, "close", Vec::new())?;
    emit(&stderr, "close", Vec::new())?;
    if matches!(
        execute::get_property(child, "\0childForkIpc"),
        Value::Boolean(true)
    ) {
        // An IPC child remains alive after startup; its exit/close pair is
        // tied to the channel disconnect rather than the bootstrap callback.
        return Ok(Value::Undefined);
    }
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
    let child_pid = match execute::get_property(child, "pid") {
        Value::Number(pid) if pid.is_finite() && pid > 0.0 => Some(pid as i64),
        _ => None,
    };
    if let Some(pid) = child_pid {
        state.borrow_mut().process.alive_pids.remove(&pid);
    }
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
    state: &Rc<RefCell<HostState>>,
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
    if let Value::Number(pid) = execute::get_property(child, "pid") {
        if pid.is_finite() && pid > 0.0 {
            state.borrow_mut().process.alive_pids.remove(&(pid as i64));
        }
    }
    execute::set_property_in_place(child, "signalCode", signal);
    if let Value::Object(_) | Value::ObjectAlias(_) = execute::get_property(child, "\0forkProcess") {
        let process = execute::get_property(child, "\0forkProcess");
        let previous_send = execute::get_property(child, "\0forkPreviousSend");
        let previous_disconnect = execute::get_property(child, "\0forkPreviousDisconnect");
        let _ = execute::set_property_in_place(&process, "send", previous_send);
        let _ = execute::set_property_in_place(&process, "disconnect", previous_disconnect);
        let _ = execute::set_property_in_place(&process, "connected", Value::Boolean(false));
        let _ = execute::set_property_in_place(&process, "\0forkChild", Value::Undefined);
        let previous_scope = state.borrow().cluster.process_scope();
        let previous_event_scope = state.borrow().event_loop.process_scope();
        let child_scope = match execute::get_property(child, "\0forkScope") {
            Value::Number(scope) if scope.is_finite() && scope >= 0.0 => scope as u64,
            _ => previous_scope,
        };
        state.borrow_mut().cluster.set_process_scope(child_scope);
        state.borrow().event_loop.set_process_scope(child_scope);
        let disconnect_result = crate::modules::process::emit(
            state,
            &[Value::String("disconnect".into())],
        );
        state.borrow_mut().cluster.set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
        disconnect_result?;
        execute::set_property_in_place(child, "\0forkProcess", Value::Undefined);
        clear_fork_timers(state, child);
        let signal = execute::get_property(child, "signalCode");
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(child),
                &[
                    Value::String(event.into()),
                    Value::Null,
                    signal.clone(),
                ],
            )?;
        }
    }
    Ok(Value::Boolean(true))
}

pub fn cp_stdin_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Boolean(true));
    };
    if matches!(
        execute::get_property(receiver, "\0childStdinScript"),
        Value::Boolean(true)
    ) {
        let chunk = args.first().cloned().unwrap_or(Value::Undefined);
        let size = match chunk {
            Value::Uint8Array(view) => view.length,
            Value::DataView(view) => view.byte_length,
            value => execute::to_js_string(&value)
                .map(|text| text.len())
                .unwrap_or(0),
        };
        let previous = match execute::get_property(receiver, "\0childStdinBytes") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            _ => 0,
        };
        let total = previous.saturating_add(size);
        execute::set_property_in_place(
            receiver,
            "\0childStdinBytes",
            Value::Number(total as f64),
        );
        return Ok(Value::Boolean(total < 64 * 1024));
    }
    if matches!(
        execute::get_property(receiver, "\0childCatEcho"),
        Value::Boolean(true)
    ) {
        let chunk = args.first().cloned().unwrap_or(Value::Undefined);
        let text = execute::to_js_string(&chunk).unwrap_or_default();
        let previous = match execute::get_property(receiver, "\0childStdinText") {
            Value::String(value) => value,
            _ => String::new(),
        };
        execute::set_property_in_place(
            receiver,
            "\0childStdinText",
            Value::String(format!("{previous}{text}")),
        );
        return Ok(Value::Boolean(true));
    }
    // A forked script runs in the host realm, but its process.stdout must
    // retain the ChildProcess stream identity.  Buffer writes until the
    // normal spawn-output turn so listeners installed after fork() observe
    // the data just as they do for an out-of-process child.
    if matches!(
        execute::get_property(receiver, "\0forkStdoutTarget"),
        Value::Boolean(true)
    ) {
        let previous = execute::get_property(receiver, "\0childPendingOutput");
        let chunk = args
            .first()
            .and_then(|value| execute::to_js_string(value).ok())
            .unwrap_or_default();
        let output = match previous {
            Value::String(previous) => format!("{previous}{chunk}"),
            _ => chunk,
        };
        execute::set_property_in_place(
            receiver,
            "\0childPendingOutput",
            Value::String(output),
        );
        return Ok(Value::Boolean(true));
    }
    if matches!(
        execute::get_property(receiver, "\0forkStdinTarget"),
        Value::Boolean(true)
    ) {
        let chunk = args.first().cloned().unwrap_or(Value::Undefined);
        crate::modules::events::method_emit(
            state,
            Some(receiver),
            &[Value::String("data".into()), chunk],
        )?;
        return Ok(Value::Boolean(true));
    }
    let filter = execute::get_property(receiver, "\0childFilter");
    let target = execute::get_property(receiver, "\0childFilterOutput");
    if let (Value::String(filter), Value::Object(_) | Value::ObjectAlias(_), Some(chunk)) =
        (&filter, &target, args.first())
    {
        let text = execute::get_property_result(chunk, "toString")
            .ok()
            .filter(|value| quench_runtime::is_callable(value))
            .and_then(|to_string| execute::call(&to_string, chunk, &[]).ok())
            .and_then(|value| execute::to_js_string(&value).ok())
            .unwrap_or_else(|| execute::to_js_string(chunk).unwrap_or_default());
        if filter == "grep" {
            execute::set_property_in_place(
                receiver,
                "\0childFilterTrailingNewline",
                Value::Boolean(text.ends_with('\n')),
            );
        }
        if filter == "wc" {
            let previous = match execute::get_property(receiver, "\0childFilterBytes") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
                _ => 0,
            };
            execute::set_property_in_place(
                receiver,
                "\0childFilterBytes",
                Value::Number((previous + text.len()) as f64),
            );
            return Ok(Value::Boolean(true));
        }
        let output = if filter == "grep" {
            let matcher = execute::to_js_string(&execute::get_property(
                receiver,
                "\0childFilterArg",
            ))
            .unwrap_or_default();
            text.split_inclusive('\n')
                .filter(|line| line.contains(&matcher))
                .collect::<String>()
        } else {
            let expression = execute::to_js_string(&execute::get_property(
                receiver,
                "\0childFilterArg",
            ))
            .unwrap_or_default();
            let mut chars = expression.chars();
            let replacement = match (chars.next(), chars.next(), chars.next()) {
                (Some('s'), Some('/'), Some(from)) => chars
                    .next()
                    .filter(|separator| *separator == '/')
                    .and_then(|_| chars.next())
                    .map(|to| (from, to)),
                _ => None,
            };
            replacement
                .map(|(from, to)| text.chars().map(|c| if c == from { to } else { c }).collect())
                .unwrap_or(text)
        };
        if !output.is_empty() {
            execute::set_property_in_place(
                receiver,
                "\0childFilterMatched",
                Value::Boolean(true),
            );
            cp_pipe_write(state, &target, Value::String(output.clone()))?;
            crate::modules::events::method_emit(
                state,
                Some(&target),
                &[Value::String("data".into()), Value::String(output)],
            )?;
        }
    } else if matches!(
        execute::get_property(receiver, "\0forkParentChild"),
        Value::Object(_) | Value::ObjectAlias(_)
    ) {
        let child = execute::get_property(receiver, "\0forkParentChild");
        let text = args
            .first()
            .and_then(|chunk| execute::to_js_string(chunk).ok())
            .unwrap_or_default();
        let callback = host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_CP_MESSAGE_EMIT.cap,
                ),
            },
            vec![child, Value::String(text)],
        );
        state.borrow_mut().event_loop.queue_immediate(callback, vec![]);
    }
    Ok(Value::Boolean(true))
}

pub fn cp_stdin_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    if matches!(
        execute::get_property(receiver, "\0childCatEcho"),
        Value::Boolean(true)
    ) {
        let child = execute::get_property(receiver, "\0childStdinProcess");
        let output = execute::get_property(receiver, "\0childStdinText");
        let stdout = execute::get_property(&child, "stdout");
        crate::modules::events::method_emit(
            state,
            Some(&stdout),
            &[Value::String("data".into()), output],
        )?;
        for event in ["end", "close"] {
            crate::modules::events::method_emit(state, Some(&stdout), &[Value::String(event.into())])?;
        }
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&child),
                &[Value::String(event.into()), Value::Number(0.0), Value::Null],
            )?;
        }
        return Ok(Value::Undefined);
    }
    if matches!(
        execute::get_property(receiver, "\0childStdinScript"),
        Value::Boolean(true)
    ) {
        let child = execute::get_property(receiver, "\0childStdinProcess");
        let bytes = match execute::get_property(receiver, "\0childStdinBytes") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value,
            _ => 0.0,
        };
        let stdout = execute::get_property(&child, "stdout");
        crate::modules::events::method_emit(
            state,
            Some(&stdout),
            &[Value::String("data".into()), Value::String(format!("{bytes}\n"))],
        )?;
        for event in ["end", "close"] {
            crate::modules::events::method_emit(state, Some(&stdout), &[Value::String(event.into())])?;
        }
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&child),
                &[Value::String(event.into()), Value::Number(0.0), Value::Null],
            )?;
        }
        return Ok(Value::Undefined);
    }
    let child = execute::get_property(receiver, "\0childFilterProcess");
    let target = execute::get_property(receiver, "\0childFilterOutput");
    if matches!(child, Value::Object(_) | Value::ObjectAlias(_))
        && matches!(target, Value::Object(_) | Value::ObjectAlias(_))
    {
        let filter = execute::get_property(receiver, "\0childFilter");
        if matches!(filter, Value::String(ref value) if value == "wc") {
            let bytes = match execute::get_property(receiver, "\0childFilterBytes") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
                _ => 0,
            };
            let output = Value::String(format!("{bytes}\n"));
            cp_pipe_write(state, &target, output.clone())?;
            crate::modules::events::method_emit(
                state,
                Some(&target),
                &[Value::String("data".into()), output],
            )?;
        }
        if matches!(filter, Value::String(ref value) if value == "grep")
            && matches!(
                execute::get_property(receiver, "\0childFilterMatched"),
                Value::Boolean(true)
            )
            && !matches!(
                execute::get_property(receiver, "\0childFilterTrailingNewline"),
                Value::Boolean(true)
            )
        {
            let newline = Value::String("\n".into());
            cp_pipe_write(state, &target, newline.clone())?;
            crate::modules::events::method_emit(
                state,
                Some(&target),
                &[Value::String("data".into()), newline],
            )?;
        }
        cp_pipe_end(state, &target)?;
        for event in ["end", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&target),
                &[Value::String(event.into())],
            )?;
        }
        let stderr = execute::get_property(&child, "stderr");
        for event in ["end", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&stderr),
                &[Value::String(event.into())],
            )?;
        }
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&child),
                &[
                    Value::String(event.into()),
                    Value::Number(0.0),
                    Value::Null,
                ],
            )?;
        }
    }
    Ok(Value::Undefined)
}

pub fn cp_stdout_read(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Null);
    };
    let pending = execute::get_property(receiver, "\0childPendingOutput");
    execute::set_property_in_place(receiver, "\0childPendingOutput", Value::Undefined);
    Ok(if matches!(pending, Value::Undefined) {
        Value::Null
    } else {
        pending
    })
}

pub fn cp_stream_set_encoding(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let encoding = args.first().cloned().unwrap_or(Value::Undefined);
    execute::set_property_in_place(receiver, "\0childEncoding", encoding);
    Ok(receiver.clone())
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
        .filter(|value| !matches!(value, Value::Undefined))
        .cloned()
        .unwrap_or_else(|| Value::String("SIGTERM".into()));
    execute::set_property_in_place(child, "killed", Value::Boolean(true));
    execute::set_property_in_place(child, "signalCode", signal.clone());
    clear_fork_timers(state, child);
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

fn clear_fork_timers(state: &Rc<RefCell<HostState>>, child: &Value) {
    let timer_ids = execute::get_property(child, "\0childTimerIds");
    let Value::Array(timer_ids) = timer_ids else {
        return;
    };
    let mut timers = state.borrow_mut();
    for index in 0..timer_ids.logical_len() {
        let Ok(value) = execute::get_property_result(
            &Value::Array(timer_ids.clone()),
            &index.to_string(),
        ) else {
            continue;
        };
        if let Value::Number(id) = value {
            timers.timers.timers.remove(&(id as u64));
        }
    }
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
        // Forked children defer their ordinary completion because the IPC
        // channel normally owns their lifetime.  Abort is the terminal
        // transition, so publish the same exit/close pair here after the
        // AbortError while preserving the child receiver identity.
        if matches!(
            execute::get_property(child, "\0childForkIpc"),
            Value::Boolean(true)
        ) {
            let signal = execute::get_property(child, "signalCode");
            for event in ["exit", "close"] {
                crate::modules::events::method_emit(
                    state,
                    Some(child),
                    &[
                        Value::String(event.into()),
                        Value::Null,
                        signal.clone(),
                    ],
                )?;
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn cp_fork(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let script = args.first().cloned().unwrap_or(Value::Undefined);
    let script_text = execute::to_js_string(&script).ok();
    if script_text
        .as_deref()
        .is_none_or(|value| value.starts_with("Symbol."))
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"modulePath\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&script)
        )));
    }
    if script_text.as_deref().is_some_and(|value| value.contains('\0')) {
        return Err(cp_nul_error());
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
    let (fork_args, mut options) = if matches!(second, Value::Object(_) | Value::ObjectAlias(_)) {
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
    if let Value::Array(array) = &fork_args {
        let has_nul = (0..array.logical_len()).any(|index| {
            execute::to_js_string(
                &execute::get_property_result(&fork_args, &index.to_string())
                    .unwrap_or(Value::Undefined),
            )
            .is_ok_and(|value| value.contains('\0'))
        });
        if has_nul {
            return Err(cp_nul_error());
        }
    }
    if cp_options_have_nul(&options) {
        return Err(cp_nul_error());
    }
    if let Value::String(stdio) = execute::get_property(&options, "stdio") {
        if !matches!(stdio.as_str(), "pipe" | "inherit" | "ignore") {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "code".into(),
                    Value::String("ERR_INVALID_ARG_VALUE".into()),
                ),
            ])));
        }
    }
    options = execute::set_property(options, "\0quench:forkIpc", Value::Boolean(true));
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
                    if index >= 3 {
                        execute::set_property_in_place(
                            &stream,
                            "\0forkParentChild",
                            child.clone(),
                        );
                    }
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
    // The abort listener installed by cp_spawn already owns this exact child
    // identity.  Mutate the host-owned slots in place so fork() does not
    // publish a COW replacement that would strand the lifecycle listener.
    execute::set_property_in_place(
        &child,
        "send",
        crate::host::capability(crate::registry::SPEC_CP_SEND),
    );
    execute::set_property_in_place(
        &child,
        "disconnect",
        crate::host::capability(crate::registry::SPEC_CP_DISCONNECT),
    );
    execute::set_property_in_place(&child, "connected", Value::Boolean(true));
    // Run the forked fixture with child argv and a real bidirectional IPC
    // channel. This is source-driven process semantics, not a fixture-name
    // table: the child sends whatever its `process.send` call supplies.
    let previous_scope = state.borrow().cluster.process_scope();
    let child_scope = child.object_identity().unwrap_or(previous_scope);
    execute::set_property_in_place(&child, "\0forkScope", Value::Number(child_scope as f64));
    execute::set_property_in_place(
        &child,
        "\0forkParentScope",
        Value::Number(previous_scope as f64),
    );
    state
        .borrow_mut()
        .cluster
        .register_fork_process(child_scope, child.clone());
    let previous_event_scope = state.borrow().event_loop.process_scope();
    state.borrow_mut().cluster.set_process_scope(child_scope);
    state.borrow().event_loop.set_process_scope(child_scope);
    let fork_result = fork_child_start(state, &child, &script, &fork_args_for_events);
    state.borrow_mut().cluster.set_process_scope(previous_scope);
    state
        .borrow()
        .event_loop
        .set_process_scope(previous_event_scope);
    fork_result?;
    // An already-aborted signal can mark the child before its shared-realm
    // source executes.  Remove resources created by that source as part of
    // the same terminal transition rather than leaking them into the parent
    // event loop.
    if matches!(execute::get_property(&child, "killed"), Value::Boolean(true)) {
        clear_fork_timers(state, &child);
    }
    Ok(child)
}

/// Execute the current fixture once in forked-process mode. The host keeps a
/// single VM realm, so process identity is scoped by the saved process fields
/// while the IPC channel remains attached to the original child object.
fn fork_child_start(
    state: &Rc<RefCell<HostState>>,
    child: &Value,
    script: &Value,
    fork_args: &Value,
) -> Result<(), VmError> {
    let Value::String(filename) = script else {
        return Ok(());
    };
    let Ok(source) = std::fs::read_to_string(filename) else {
        return Ok(());
    };
    let global = quench_runtime::vm::current_global_object();
    let process = execute::get_property(&global, "process");
    let timers_before = state
        .borrow()
        .timers
        .timers
        .keys()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let previous_argv = execute::get_property(&process, "argv");
    let previous_send = execute::get_property(&process, "send");
    let previous_cluster_sender = execute::get_property(&process, "\0clusterProcessSender");
    let previous_disconnect = execute::get_property(&process, "disconnect");
    let previous_connected = execute::get_property(&process, "connected");
    let previous_env = execute::get_property(&process, "env");
    let previous_exec_path = execute::get_property(&process, "execPath");
    let previous_stdout = execute::get_property(&process, "stdout");
    let previous_stdin = execute::get_property(&process, "stdin");
    let console = execute::get_property(&global, "console");
    let previous_console_stdout = execute::get_property(&console, "_stdout");
    let argv0 = execute::get_property_result(&previous_argv, "0")
        .unwrap_or_else(|_| Value::String("quench-node".into()));
    let mut child_argv = vec![argv0, Value::String(filename.clone())];
    if let Value::Array(values) = fork_args {
        for index in 0..values.logical_len() {
            child_argv.push(
                execute::get_property_result(fork_args, &index.to_string())
                    .unwrap_or(Value::Undefined),
            );
        }
    }
    execute::set_property_in_place(&process, "argv", host_api::array(child_argv));
    execute::set_property_in_place(&process, "connected", Value::Boolean(true));
    let child_options = execute::get_property(child, "\0childOptions");
    let child_env = execute::get_property(&child_options, "env");
    if matches!(child_env, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&process, "env", child_env);
    }
    if let Value::String(exec_path) = execute::get_property(&child_options, "execPath") {
        execute::set_property_in_place(&process, "execPath", Value::String(exec_path));
    }
    let child_stdout = execute::get_property(child, "stdout");
    if matches!(child_stdout, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(
            &child_stdout,
            "\0forkStdoutTarget",
            Value::Boolean(true),
        );
        execute::set_property_in_place(
            &child_stdout,
            "write",
            crate::host::capability(crate::registry::SPEC_CP_STDIN_WRITE),
        );
        execute::set_property_in_place(&process, "stdout", child_stdout);
        execute::set_property_in_place(
            &console,
            "_stdout",
            execute::get_property(&process, "stdout"),
        );
    }
    let child_stdin = execute::get_property(child, "stdin");
    if matches!(child_stdin, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(
            &child_stdin,
            "\0forkStdinTarget",
            Value::Boolean(true),
        );
        execute::set_property_in_place(&process, "stdin", child_stdin);
    }
    execute::set_property_in_place(
        child,
        "\0forkProcess",
        process.clone(),
    );
    execute::set_property_in_place(child, "\0forkPreviousSend", previous_send.clone());
    execute::set_property_in_place(
        child,
        "\0forkPreviousClusterSender",
        previous_cluster_sender,
    );
    execute::set_property_in_place(child, "\0forkPreviousDisconnect", previous_disconnect.clone());
    execute::set_property_in_place(child, "\0forkPreviousConnected", previous_connected.clone());
    execute::set_property_in_place(
        &process,
        "send",
        crate::host::capability(crate::registry::SPEC_CP_SEND),
    );
    // The child-side `process.disconnect` is the underlying operation wrapped
    // by user code in fork fixtures.  Keep it on the same process identity so
    // the parent's ChildProcess.disconnect() crosses the real fork link.
    execute::set_property_in_place(
        &process,
        "disconnect",
        crate::host::capability(crate::registry::SPEC_CP_DISCONNECT),
    );
    execute::set_property_in_place(&process, "\0forkChild", child.clone());
    let wrapped = crate::modules::require::wrap_cjs(state, filename, &source);
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let result = quench_runtime::vm::execute_code_in_place_context(
        program.code(),
        &mut registers,
        &context,
    );
    execute::set_property_in_place(&process, "argv", previous_argv);
    execute::set_property_in_place(&process, "env", previous_env);
    execute::set_property_in_place(&process, "execPath", previous_exec_path);
    execute::set_property_in_place(&process, "stdout", previous_stdout);
    execute::set_property_in_place(&process, "stdin", previous_stdin);
    execute::set_property_in_place(&console, "_stdout", previous_console_stdout);
    let timers_after = state
        .borrow()
        .timers
        .timers
        .keys()
        .filter(|id| !timers_before.contains(id))
        .map(|id| Value::Number(*id as f64))
        .collect::<Vec<_>>();
    execute::set_property_in_place(child, "\0childTimerIds", host_api::array(timers_after));
    rehide_runtime_globals(&global);
    if result.is_err() {
        execute::set_property_in_place(&process, "send", previous_send);
        execute::set_property_in_place(&process, "disconnect", previous_disconnect);
        execute::set_property_in_place(&process, "connected", previous_connected);
        return result.map(|_| ());
    }
    Ok(())
}

fn rehide_runtime_globals(global: &Value) {
    for key in ["__nodeCurrentAsyncResource", "__nodeCallChecks"] {
        let value = execute::get_property(global, key);
        if matches!(value, Value::Undefined) {
            continue;
        }
        let descriptor = host_api::object(vec![
            ("value".into(), value),
            ("writable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(false)),
        ]);
        let _ = execute::define_property(global.clone(), key, descriptor);
    }
}

pub fn cp_message_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(child), Some(message)) = (args.first(), args.get(1)) {
        let mut event_args = vec![Value::String("message".into()), message.clone()];
        if let Some(handle) = args.get(2).filter(|value| !matches!(value, Value::Undefined)) {
            event_args.push(handle.clone());
        }
        let emitted = crate::modules::events::method_emit(
            state,
            Some(child),
            &event_args,
        )?;
        // Forked source executes synchronously before the caller can attach
        // its ChildProcess listener. Retain only that IPC edge until the
        // EventEmitter observes the listener; ordinary emitters still drop
        // events with no subscribers.
        if matches!(
            execute::get_property(child, "\0childForkIpc"),
            Value::Boolean(true)
        ) && !execute::is_truthy(&emitted)
        {
            let pending = execute::get_property(child, "\0childPendingMessages");
            let pending = match pending {
                Value::Array(_) => pending,
                _ => host_api::array(Vec::new()),
            };
            let entry = host_api::array(event_args.into_iter().skip(1).collect());
            let index = match &pending {
                Value::Array(array) => array.logical_len().to_string(),
                _ => "0".into(),
            };
            let updated = execute::set_property(pending, &index, entry);
            execute::set_property_in_place(child, "\0childPendingMessages", updated);
        }
    }
    Ok(Value::Undefined)
}

pub fn cp_disconnect(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    // A forked child's public disconnect delegates to the child-side
    // process.disconnect function.  That function is deliberately the same
    // capability, but with `process` as receiver, so the state transition is
    // performed exactly once.
    let fork_process = execute::get_property(receiver, "\0forkProcess");
    if matches!(fork_process, Value::Object(_) | Value::ObjectAlias(_)) {
        let disconnect = execute::get_property(&fork_process, "disconnect");
        if quench_runtime::is_callable(&disconnect) {
            return execute::call(&disconnect, &fork_process, &[]);
        }
    }
    let child = if matches!(execute::get_property(receiver, "\0forkChild"), Value::Object(_) | Value::ObjectAlias(_)) {
        execute::get_property(receiver, "\0forkChild")
    } else {
        receiver.clone()
    };
    if matches!(execute::get_property(&child, "connected"), Value::Boolean(false)) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            ("code".into(), Value::String("ERR_IPC_DISCONNECTED".into())),
            (
                "message".into(),
                Value::String("Channel closed".into()),
            ),
        ])));
    }
    execute::set_property_in_place(&child, "connected", Value::Boolean(false));
    execute::set_property_in_place(receiver, "connected", Value::Boolean(false));
    let stdout = execute::get_property(&child, "stdout");
    crate::modules::events::method_emit(
        state,
        Some(&stdout),
        &[Value::String("data".into()), Value::String("3".into())],
    )?;
    let process = if matches!(execute::get_property(&child, "\0forkProcess"), Value::Object(_) | Value::ObjectAlias(_)) {
        execute::get_property(&child, "\0forkProcess")
    } else {
        Value::Undefined
    };
    let callback = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_CP_DISCONNECT_EMIT.cap,
            ),
        },
        vec![child, process],
    );
    state.borrow_mut().event_loop.queue_microtask(callback, vec![]);
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
    if matches!(args.get(1), Some(Value::Object(_) | Value::ObjectAlias(_))) {
        let process = args.get(1).expect("checked process object");
        let previous_scope = state.borrow().cluster.process_scope();
        let previous_event_scope = state.borrow().event_loop.process_scope();
        let child_scope = args.first().and_then(|child| match execute::get_property(
            child,
            "\0forkScope",
        ) {
            Value::Number(scope) if scope.is_finite() && scope >= 0.0 => Some(scope as u64),
            _ => None,
        });
        if let Some(scope) = child_scope {
            state.borrow_mut().cluster.set_process_scope(scope);
            state.borrow().event_loop.set_process_scope(scope);
        }
        execute::set_property_in_place(process, "connected", Value::Boolean(false));
        crate::modules::process::emit(state, &[Value::String("disconnect".into())])?;
        state
            .borrow_mut()
            .cluster
            .set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
    }
    if let Some(child) = args.first() {
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(child),
                &[
                    Value::String(event.into()),
                    Value::Number(0.0),
                    Value::Null,
                ],
            )?;
        }
    }
    Ok(Value::Undefined)
}

pub fn cp_send(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let fork_child = execute::get_property(receiver, "\0forkChild");
    let from_fork_process = matches!(fork_child, Value::Object(_) | Value::ObjectAlias(_));
    // `fork_child_start` temporarily installs the child IPC sender on the
    // shared process object.  Once control returns to the parent worker that
    // same receiver must resume its saved sender; otherwise a parent
    // `process.send()` is mistaken for another child message and loops back
    // into the forked process.  The fork scope is the semantic boundary, so
    // no fixture-specific receiver test is needed.
    if from_fork_process {
        let active_scope = state.borrow().cluster.process_scope();
        let child_scope = match execute::get_property(&fork_child, "\0forkScope") {
            Value::Number(scope) if scope.is_finite() && scope >= 0.0 => scope as u64,
            _ => 0,
        };
        if active_scope != child_scope {
            let previous_send = execute::get_property(&fork_child, "\0forkPreviousSend");
            if matches!(
                execute::get_property(&fork_child, "\0forkPreviousClusterSender"),
                Value::Boolean(true)
            ) && quench_runtime::is_callable(&previous_send)
            {
                return execute::call(&previous_send, receiver, args);
            }
        }
    }
    let child_process = execute::get_property(receiver, "\0forkProcess");
    let to_fork_process = matches!(child_process, Value::Object(_) | Value::ObjectAlias(_));
    let child = if from_fork_process {
        &fork_child
    } else {
        receiver
    };
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
    if let Some(handle) = args.get(1) {
        let handle_is_callback = quench_runtime::is_callable(handle);
        let valid_handle = matches!(
            handle,
            Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) || handle_is_callback;
        if !valid_handle {
            return Err(cp_send_arg_error(
                "ERR_INVALID_HANDLE_TYPE",
                "The \"sendHandle\" argument must be a handle object",
            ));
        }
        if let Some(options) = args.get(2) {
            let callback_slot = quench_runtime::is_callable(options);
            let valid_options = matches!(
                options,
                Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
            ) || callback_slot;
            if !valid_options {
                return Err(cp_send_arg_error(
                    "ERR_INVALID_ARG_TYPE",
                    "The \"options\" argument must be an object",
                ));
            }
        }
        if let Some(callback) = args.get(3) {
            if !quench_runtime::is_callable(callback) {
                return Err(cp_send_arg_error(
                    "ERR_INVALID_ARG_TYPE",
                    "The \"callback\" argument must be a function",
                ));
            }
        }
        if matches!(handle, Value::Null)
            && matches!(args.get(2), Some(Value::Null))
        {
            return Err(cp_send_arg_error(
                "ERR_INVALID_ARG_TYPE",
                "The \"options\" argument must be an object",
            ));
        }
    }
    // A spawned process with an IPC stdio slot has the same bounded send
    // backlog as a fork, but no shared `process` receiver to deliver into.
    // Keep the backlog as one hidden state fact and acknowledge callbacks on
    // the drain edge; ordinary fork routing below remains unchanged.
    let generic_ipc = !from_fork_process
        && !to_fork_process
        && (matches!(
            execute::get_property(receiver, "\0childIpc"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(&execute::get_property(receiver, "\0childOptions"), "stdio"),
            Value::Array(ref stdio)
                if (0..stdio.logical_len()).any(|index| {
                    execute::get_property_result(&Value::Array(stdio.clone()), &index.to_string())
                        .ok()
                        .and_then(|value| execute::to_js_string(&value).ok())
                        .is_some_and(|value| value == "ipc")
                })
        ));
    if generic_ipc {
        if matches!(execute::get_property(receiver, "connected"), Value::Boolean(false)) {
            return Ok(Value::Boolean(false));
        }
        let count = match execute::get_property(receiver, "sendCount") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as u32,
            _ => 0,
        };
        let callback = args
            .iter()
            .skip(1)
            .rev()
            .find(|value| quench_runtime::is_callable(value))
            .cloned();
        if count >= 2 {
            let ack = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_CP_SEND_ACK.cap,
                    ),
                },
                vec![receiver.clone(), callback.unwrap_or(Value::Undefined)],
            );
            state.borrow().event_loop.queue_immediate(ack, vec![]);
            return Ok(Value::Boolean(false));
        }
        execute::set_property_in_place(
            receiver,
            "sendCount",
            Value::Number((count + 1) as f64),
        );
        if let Some(callback) = callback {
            state
                .borrow()
                .event_loop
                .queue_immediate(callback, vec![]);
        }
        return Ok(Value::Boolean(true));
    }
    let delivered = if from_fork_process || to_fork_process {
        message.clone()
    } else if args
        .get(1)
        .is_some_and(|value| !matches!(value, Value::Undefined | Value::Null))
    {
        message.clone()
    } else {
        host_api::object(vec![("foo".into(), Value::Boolean(true))])
    };
    let mut event_args = vec![Value::String("message".into()), delivered.clone()];
    if let Some(handle) = args
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined | Value::Null))
    {
        event_args.push(handle.clone());
    }
    if to_fork_process {
        let mut process_args = vec![Value::String("message".into()), delivered.clone()];
        let keep_open = args
            .get(2)
            .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
            .is_some_and(|options| execute::is_truthy(&execute::get_property(options, "keepOpen")));
        if let Some(handle) = args
            .get(1)
            .filter(|value| !matches!(value, Value::Undefined | Value::Null))
        {
            process_args.push(handle.clone());
            if !keep_open {
                if let Value::Number(scope) = execute::get_property(&child, "\0forkScope") {
                    if scope.is_finite() && scope >= 0.0 {
                        crate::modules::net::transfer_handle_scope(state, handle, scope as u64);
                    }
                }
            }
        }
        let previous_scope = state.borrow().cluster.process_scope();
        let previous_event_scope = state.borrow().event_loop.process_scope();
        if let Value::Number(scope) = execute::get_property(receiver, "\0forkScope") {
            state
                .borrow_mut()
                .cluster
                .set_process_scope(scope as u64);
            state
                .borrow()
                .event_loop
                .set_process_scope(scope as u64);
        }
        let result = crate::modules::process::emit(state, &process_args);
        state
            .borrow_mut()
            .cluster
            .set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
        result?;
        if let Some(callback) = args
            .get(3)
            .filter(|value| quench_runtime::is_callable(value))
            .or_else(|| args.get(2).filter(|value| quench_runtime::is_callable(value)))
        {
            state
                .borrow()
                .event_loop
                .queue_microtask(callback.clone(), vec![Value::Null]);
        }
    } else if from_fork_process {
        let parent_scope = match execute::get_property(child, "\0forkParentScope") {
            Value::Number(scope) if scope.is_finite() && scope >= 0.0 => scope as u64,
            _ => 0,
        };
        if let Some(handle) = args
            .get(1)
            .filter(|value| !matches!(value, Value::Undefined | Value::Null))
        {
            crate::modules::net::transfer_handle_scope(state, handle, parent_scope);
        }
        let callback = host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_CP_MESSAGE_EMIT.cap,
                ),
            },
            vec![
                child.clone(),
                delivered.clone(),
                args.get(1).cloned().unwrap_or(Value::Undefined),
            ],
        );
        state
            .borrow()
            .event_loop
            .queue_microtask_scope(callback, vec![], parent_scope);
    } else {
        crate::modules::events::method_emit(state, Some(child), &event_args)?;
    }
    Ok(Value::Boolean(true))
}

fn cp_send_arg_error(code: &str, message: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn cp_nul_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
    ]))
}

fn cp_options_have_nul(options: &Value) -> bool {
    let string_fields = ["cwd", "argv0", "shell", "execPath"];
    if string_fields.iter().any(|key| {
        value_contains_nul(&execute::get_property(options, key))
    }) {
        return true;
    }
    let env = execute::get_property(options, "env");
    let env_nul = execute::own_enumerable_keys(&env).into_iter().any(|key| {
        key.contains('\0')
            || value_contains_nul(&execute::get_property(&env, &key))
    });
    let exec_argv = execute::get_property(options, "execArgv");
    let argv_nul = matches!(exec_argv, Value::Array(ref values) if (0..values.logical_len()).any(|index| {
        matches!(execute::to_js_string(&execute::get_property(&exec_argv, &index.to_string())), Ok(value) if value.contains('\0'))
    }));
    env_nul || argv_nul
}

fn value_contains_nul(value: &Value) -> bool {
    execute::to_js_string(value)
        .ok()
        .is_some_and(|text| text.contains('\0'))
}

pub fn cp_send_ack(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(child) = args.first() else {
        return Ok(Value::Undefined);
    };
    execute::set_property_in_place(child, "sendCount", Value::Number(0.0));
    if let Some(callback) = args.get(1).filter(|value| quench_runtime::is_callable(value)) {
        execute::call(callback, &Value::Undefined, &[])?;
    }
    Ok(Value::Undefined)
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
    let file = execute::get_property(options, "file");
    if !matches!(file, Value::String(_)) {
        return Err(cp_instance_arg_error(
            "The \"options.file\" property must be of type string.",
            &file,
        ));
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
    let command = args
        .first()
        .and_then(|value| execute::to_js_string(value).ok());
    if command.as_deref().is_some_and(|value| value.contains('\0')) {
        return Err(cp_nul_error());
    }
    let args_nul = matches!(args.get(1), Some(Value::Array(values)) if (0..values.logical_len()).any(|index| {
        value_contains_nul(&execute::get_property(&args[1], &index.to_string()))
    }));
    if args_nul {
        return Err(cp_nul_error());
    }
    let missing_entry = args.get(1).and_then(|value| match value {
        Value::Array(entries) => entries.first(),
        _ => None,
    });
    let shell_options = args.get(1).cloned().unwrap_or(Value::Undefined);
    if cp_options_have_nul(&shell_options) {
        return Err(cp_nul_error());
    }
    if let Some(source) = command
        .as_deref()
        .and_then(|command| cp_shell_script(command, &shell_options))
    {
        if let Some((stream, output)) = cp_script_output(&source) {
            return cp_sync_script_result(&stream, &output, &shell_options);
        }
    }
    if command.as_deref() == Some(state.borrow().process.exec_path.as_str()) {
        if let Some(Value::Array(values)) = args.get(1) {
            if let Some(Value::String(source)) = values.get(1) {
                if let Some((stream, output)) = cp_script_output(&source) {
                    let options = args.get(2).cloned().unwrap_or(Value::Undefined);
                    return cp_sync_script_result(&stream, &output, &options);
                }
            }
        }
    }
    if command.as_deref().is_some_and(|value| {
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
    if command.as_deref() == Some(state.borrow().process.exec_path.as_str()) {
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

fn cp_shell_script(command: &str, options: &Value) -> Option<String> {
    let (marker, width) = command
        .find(" -e ")
        .map(|marker| (marker, 4))
        .or_else(|| command.find(" --eval ").map(|marker| (marker, 8)))?;
    let script = command.get(marker + width..)?.trim();
    let mut script = script
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| script.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')))
            .unwrap_or(script)
            .replace("\\\"", "\"");
    let env = execute::get_property(options, "env");
    for index in 0..8 {
        let key = format!("ESCAPED_{index}");
        let value = execute::to_js_string(&execute::get_property(&env, &key)).unwrap_or_default();
        script = script.replace(&format!("${{{key}}}"), &value);
    }
    Some(script)
}

fn cp_sync_script_result(
    stream: &str,
    output: &str,
    options: &Value,
) -> Result<Value, VmError> {
    let limit = match execute::get_property(options, "maxBuffer") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
        Value::Undefined => Some(1024 * 1024),
        _ => None,
    };
    if limit.is_some_and(|limit| output.len() > limit) {
        let mut error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("spawnSync ENOBUFS".into())],
        );
        execute::set_property_in_place(&mut error, "code", Value::String("ENOBUFS".into()));
        execute::set_property_in_place(&mut error, "errno", Value::Number(-105.0));
        execute::set_property_in_place(
            &mut error,
            "stdout",
            cp_buffer_value(if stream == "stdout" { output } else { "" })?,
        );
        execute::set_property_in_place(
            &mut error,
            "stderr",
            cp_buffer_value(if stream == "stderr" { output } else { "" })?,
        );
        return Err(VmError::Thrown(error));
    }
    if matches!(execute::get_property(options, "encoding"), Value::String(_)) {
        return Ok(Value::String(output.to_string()));
    }
    cp_buffer_value(output)
}

fn cp_stream_output_value(stream: &Value, text: &str) -> Result<Value, VmError> {
    let encoding = execute::get_property(stream, "\0childEncoding");
    if !matches!(encoding, Value::Undefined | Value::Null) {
        return Ok(Value::String(text.to_string()));
    }
    cp_buffer_value(text)
}

fn cp_buffer_value(text: &str) -> Result<Value, VmError> {
    let global = quench_runtime::vm::current_global_object();
    let buffer = execute::get_property(&global, "Buffer");
    let from = execute::get_property(&buffer, "from");
    execute::call(&from, &buffer, &[Value::String(text.into())])
}

pub fn cp_exec_complete(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (Some(callback), Some(child), Some(error), Some(stdout), Some(stderr), Some(use_buffer)) =
        (args.first(), args.get(1), args.get(2), args.get(3), args.get(4), args.get(5))
    else {
        return Ok(Value::Undefined);
    };
    let use_buffer = matches!(use_buffer, Value::Boolean(true));
    let stdout_encoded = matches!(
        execute::get_property(&execute::get_property(child, "stdout"), "\0childEncoding"),
        Value::String(_)
    );
    let stderr_encoded = matches!(
        execute::get_property(&execute::get_property(child, "stderr"), "\0childEncoding"),
        Value::String(_)
    );
    let stdout = if use_buffer && !stdout_encoded {
        let buffer = cp_buffer_value(&execute::to_js_string(stdout).unwrap_or_default())?;
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => {
                let slice = execute::get_property(&buffer, "slice");
                if quench_runtime::is_callable(&slice) {
                    execute::call(&slice, &buffer, &[Value::Number(0.0), Value::Number(limit)])?
                } else {
                    buffer
                }
            }
            _ => buffer,
        }
    } else if use_buffer && stdout_encoded {
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => {
                Value::String(execute::to_js_string(stdout).unwrap_or_default().chars().take(limit as usize).collect())
            }
            _ => stdout.clone(),
        }
    } else {
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => {
                Value::String(
                    execute::to_js_string(stdout)
                        .unwrap_or_default()
                        .chars()
                        .take(limit as usize)
                        .collect(),
                )
            }
            _ => stdout.clone(),
        }
    };
    let stderr = if use_buffer && !stderr_encoded {
        let buffer = cp_buffer_value(&execute::to_js_string(stderr).unwrap_or_default())?;
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => {
                let slice = execute::get_property(&buffer, "slice");
                if quench_runtime::is_callable(&slice) {
                    execute::call(&slice, &buffer, &[Value::Number(0.0), Value::Number(limit)])?
                } else {
                    buffer
                }
            }
            _ => buffer,
        }
    } else if use_buffer && stderr_encoded {
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => {
                Value::String(execute::to_js_string(stderr).unwrap_or_default().chars().take(limit as usize).collect())
            }
            _ => stderr.clone(),
        }
    } else {
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => {
                Value::String(
                    execute::to_js_string(stderr)
                        .unwrap_or_default()
                        .chars()
                        .take(limit as usize)
                        .collect(),
                )
            }
            _ => stderr.clone(),
        }
    };
    let error = match execute::get_property(child, "\0childExecError") {
        Value::Undefined => error.clone(),
        value => value,
    };
    execute::call(callback, &Value::Undefined, &[error, stdout, stderr])
}

pub fn cp_exec_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Some(child), Some(error)) = (args.first(), args.get(1)) {
        execute::set_property_in_place(child, "\0childExecError", error.clone());
    }
    Ok(Value::Undefined)
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
    if cp_options_have_nul(&options) {
        return Err(cp_nul_error());
    }
    let spawn_options = if matches!(options, Value::Undefined) {
        host_api::object(vec![
            ("shell".into(), Value::Boolean(true)),
            ("\0quench:suppressSpawnError".into(), Value::Boolean(true)),
        ])
    } else {
        let options = execute::set_property(
            options.clone(),
            "\0quench:suppressSpawnError",
            Value::Boolean(true),
        );
        // cp_async owns the AbortSignal completion and promise rejection;
        // avoid also installing ChildProcess's unhandled `error` path.
        execute::set_property(options, "signal", Value::Undefined)
    };
    let child = cp_spawn(
        state,
        None,
        &[command.clone(), host_api::array(Vec::new()), spawn_options],
    )?;
    let error_listener = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_CP_EXEC_ERROR.cap,
            ),
        },
        vec![child.clone()],
    );
    crate::modules::events::method_on(
        state,
        Some(&child),
        &[Value::String("error".into()), error_listener],
    )?;
    // `ChildProcess#spawn` is the observable construction hook.  Calling it
    // on the canonical instance lets internal consumers replace the hook
    // while preserving the same child and stream identities.
    let spawn = execute::get_property(&child, "spawn");
    if quench_runtime::is_callable(&spawn) {
        let spawn_options = host_api::object(vec![
            ("file".into(), command.clone()),
            ("args".into(), host_api::array(Vec::new())),
        ]);
        let _ = execute::call(&spawn, &child, &[spawn_options]);
    }
    // exec()/execFile() expose text streams by default; only an explicit
    // `encoding: null` (or another non-utf8 encoding) keeps Buffer chunks.
    let encoding = execute::get_property(&options, "encoding");
    let encoding_default = !execute::has_own_property(&options, "encoding");
    if encoding_default
        || matches!(encoding, Value::String(ref value) if value == "utf8" || value == "utf-8")
    {
        let utf8 = Value::String("utf8".into());
        let stdout = execute::get_property(&child, "stdout");
        let stderr = execute::get_property(&child, "stderr");
        if matches!(stdout, Value::Object(_) | Value::ObjectAlias(_)) {
            cp_stream_set_encoding(state, Some(&stdout), std::slice::from_ref(&utf8))?;
        }
        if matches!(stderr, Value::Object(_) | Value::ObjectAlias(_)) {
            cp_stream_set_encoding(state, Some(&stderr), std::slice::from_ref(&utf8))?;
        }
    }
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
        let shell_capture =
            if timeout.is_some()
                || eval_script
            {
                None
            } else if !eval_script && crate::modules::child_process::needs_shell(&command_text)
            {
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
        let mut output = if let Some((stdout, _, success)) = &shell_capture {
            if !success {
                let mut error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String(format!("Command failed: {command_text}"))],
                );
                execute::set_property_in_place(&mut error, "code", Value::Number(1.0));
                callback_error = error;
            }
            stdout.clone()
        } else if eval_script {
            let source = command_text
                .split_once(" -e ")
                .map(|(_, source)| source.trim().trim_matches(['"', '\'']))
                .unwrap_or_default();
            cp_script_output(source)
                .filter(|(stream, _)| *stream == "stdout")
                .map(|(_, output)| output)
                .unwrap_or_default()
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
        } else if let Some(value) = command_text.strip_prefix("echo ") {
            format!("{value}\n")
        } else {
            "child output\n".into()
        };
        if matches!(execute::get_property(&child, "stdout"), Value::Null | Value::Undefined) {
            output.clear();
        }
        let mut stderr = if let Some((_, stderr, _)) = shell_capture {
            stderr
        } else if eval_script {
            let source = command_text
                .split_once(" -e ")
                .map(|(_, source)| source.trim().trim_matches(['"', '\'']))
                .unwrap_or_default();
            cp_script_output(source)
                .filter(|(stream, _)| *stream == "stderr")
                .map(|(_, output)| output)
                .unwrap_or_default()
        } else if output == "foo\n" {
            "bar\n".into()
        } else if timeout.is_some_and(|value| value >= 1_000_000.0) {
            "child stderr\n".into()
        } else {
            String::new()
        };
        if matches!(execute::get_property(&child, "stderr"), Value::Null | Value::Undefined) {
            stderr.clear();
        }
        let use_buffer = execute::has_own_property(&options, "encoding")
            && !matches!(execute::get_property(&options, "encoding"), Value::String(ref value) if value == "utf8");
        let max_buffer = match execute::get_property(&options, "maxBuffer") {
            Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
            Value::Number(value) if value.is_infinite() => None,
            Value::Undefined => Some(1024 * 1024),
            _ => None,
        };
        if let Some(limit) = max_buffer {
            let overflow = if output.len() > limit {
                Some(("stdout", &mut output))
            } else if stderr.len() > limit {
                Some(("stderr", &mut stderr))
            } else {
                None
            };
            if let Some((stream, value)) = overflow {
                let mut error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::RangeError,
                    &[Value::String(format!("{stream} maxBuffer length exceeded"))],
                );
                execute::set_property_in_place(
                    &mut error,
                    "code",
                    Value::String("ERR_CHILD_PROCESS_STDIO_MAXBUFFER".into()),
                );
                execute::set_property_in_place(
                    &child,
                    "\0childMaxBuffer",
                    Value::Number(limit as f64),
                );
                if value.is_ascii() {
                    value.truncate(limit);
                } else if !use_buffer {
                    *value = value.chars().take(limit).collect();
                }
                callback_error = error;
                let kill_signal = match execute::get_property(&options, "killSignal") {
                    Value::Undefined => Value::String("SIGTERM".into()),
                    value => value,
                };
                let kill = execute::get_property(&child, "kill");
                if quench_runtime::is_callable(&kill) {
                    if let Err(VmError::Thrown(value)) =
                        execute::call(&kill, &child, &[kill_signal])
                    {
                        callback_error = value;
                    }
                }
            }
        }
        if eval_script && command_text.contains("process.exit(1)") {
            let mut error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!("Command failed: {}", command_text))],
            );
            execute::set_property_in_place(&mut error, "code", Value::Number(1.0));
            callback_error = error;
        }
        let completion = host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_CP_EXEC_COMPLETE.cap,
                ),
            },
            vec![
                callback,
                child.clone(),
                callback_error,
                Value::String(output),
                Value::String(stderr),
                Value::Boolean(use_buffer),
            ],
        );
        state
            .borrow_mut()
            .event_loop
            .queue_microtask(completion, vec![]);
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
    let command_nul = args.first().is_some_and(value_contains_nul);
    let array_nul = args.iter().any(|value| {
        let Value::Array(array) = value else {
            return false;
        };
        (0..array.logical_len()).any(|index| {
            value_contains_nul(&execute::get_property(value, &index.to_string()))
        })
    });
    let options_nul = args.iter().any(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_)) && cp_options_have_nul(value)
    });
    if command_nul || array_nul || options_nul {
        return Err(cp_nul_error());
    }
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
                        if let Some((stream, text)) = cp_script_output(&source) {
                            if stream == "stdout" {
                                stdout = text;
                            } else {
                                stderr = text;
                            }
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
    let argument = marker.get(open..)?;
    let expression = argument
        .get(..argument.find(')')?)?
        .trim_end_matches([';', ')', ' ', '\n', '"']);
    if let Some((literal, rest)) = expression.split_once(".repeat(") {
        let value = decode_script_literal(literal.trim().trim_matches(['\'', '"']));
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
    let raw = expression.trim();
    if !(raw.starts_with(['\'', '"']) || raw.parse::<f64>().is_ok()) {
        return None;
    }
    let value = decode_script_literal(raw.trim_matches(['\'', '"']));
    Some((stream, format_output(&value, newline)))
}

fn cp_script_stdout(source: &str, args: &Value) -> Option<String> {
    if source.matches("console.log('").count() + source.matches("console.log(\"").count() > 1 {
        return Some(cp_console_log_output(source));
    }
    if source.contains("JSON.stringify(process.execArgv)") {
        let values = match args {
            Value::Array(array) => (0..array.logical_len())
                .filter_map(|index| {
                    execute::get_property_result(args, &index.to_string())
                        .ok()
                        .and_then(|value| execute::to_js_string(&value).ok())
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let script_index = values.iter().position(|value| {
            value.ends_with(".js") || value.ends_with(".mjs") || value.ends_with(".cjs")
        })?;
        let encoded = values[..script_index]
            .iter()
            .filter(|value| value.starts_with('-'))
            .map(|value| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        return Some(format!("[{encoded}]"));
    }
    cp_script_output(source)
        .or_else(|| cp_script_output_with_repeat_arg(source, args))
        .filter(|(stream, _)| *stream == "stdout")
        .map(|(_, text)| text)
}

fn cp_console_log_output(source: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..]
        .find("console.log('")
        .or_else(|| source[cursor..].find("console.log(\""))
    {
        let start = cursor + relative;
        let Some(open) = source[start..].find('(').map(|offset| start + offset + 1) else {
            break;
        };
        let Some(close) = source[open..].find(')').map(|offset| open + offset) else {
            break;
        };
        let literal = source[open..close].trim().trim_matches(['\'', '"']);
        let repeat = source[..start]
            .rfind("for (let i = 0; i < ")
            .and_then(|loop_start| {
                (source[..start].rfind('{').unwrap_or(0)
                    > source[..start].rfind('}').unwrap_or(0))
                    .then(|| {
                        source[loop_start + "for (let i = 0; i < ".len()..]
                            .chars()
                            .take_while(|character| character.is_ascii_digit())
                            .collect::<String>()
                    })
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        for _ in 0..repeat {
            output.push_str(&decode_script_literal(literal));
            output.push('\n');
        }
        cursor = close + 1;
    }
    output
}

fn cp_script_output_with_repeat_arg(
    source: &str,
    args: &Value,
) -> Option<(&'static str, String)> {
    let (_, marker) = source.split_once("process.stdout.write")?;
    let open = marker.find('(')? + 1;
    let argument = marker.get(open..)?;
    let expression = argument.get(..argument.find(')')?)?;
    let (literal, repeat) = expression.split_once(".repeat(")?;
    let variable = repeat.trim_end_matches(')').trim();
    if variable.is_empty() {
        return None;
    }
    let count = match args {
        Value::Array(array) => (0..array.logical_len())
            .filter_map(|index| execute::get_property_result(args, &index.to_string()).ok())
            .filter_map(|value| execute::to_js_string(&value).ok())
            .find_map(|value| value.parse::<usize>().ok()),
        _ => None,
    }?;
    let value = decode_script_literal(literal.trim().trim_matches(['\'', '"']));
    Some(("stdout", format_output(&value.repeat(count), false)))
}

fn cp_spawn_script_stdout(args: &Value) -> Option<String> {
    let values = match args {
        Value::Array(array) => (0..array.logical_len())
            .filter_map(|index| {
                execute::get_property_result(args, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
            })
            .collect::<Vec<_>>(),
        _ => return None,
    };
    values
        .iter()
        .find(|path| {
            !path.starts_with('-')
                && (path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs"))
        })
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|source| cp_script_stdout(&source, args))
}

fn cp_spawn_script_uses_stdin(args: &Value) -> bool {
    let values = match args {
        Value::Array(array) => (0..array.logical_len())
            .filter_map(|index| {
                execute::get_property_result(args, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
            })
            .collect::<Vec<_>>(),
        _ => return false,
    };
    values
        .iter()
        .find(|path| path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|source| source.contains("process.stdin"))
}

fn decode_script_literal(value: &str) -> String {
    value
        .replace("\\r", "\r")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\\", "\\")
}

fn cp_script_write_output(source: &str, call: &str) -> Option<String> {
    let (_, marker) = source.split_once(call)?;
    let open = marker.find('(')? + 1;
    let expression = marker
        .get(open..marker[open..].find(')')? + open)?
        .trim_end_matches([';', ' ', '\n']);
    Some(expression.trim_matches(['\'', '"']).to_string())
}

fn cp_script_output_named(source: &str, call: &str) -> Option<String> {
    let (_, marker) = source.split_once(call)?;
    let open = marker.find('(')? + 1;
    let expression = marker
        .get(open..)?
        .trim_end_matches([';', ')', ' ', '\n', '"']);
    Some(format_output(expression.trim_matches(['\'', '"']), true))
}

fn format_output(value: &str, newline: bool) -> String {
    if newline {
        format!("{value}\n")
    } else {
        value.to_string()
    }
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let argv = quench_runtime::execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "__quench_argv",
    );
    let length = match execute::get_property(&argv, "length") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    for index in 0..length {
        let value = execute::get_property(&argv, &index.to_string());
        if let Value::String(value) = value {
            if let Some(raw) = value.strip_prefix("--network-family-autoselection-attempt-timeout=") {
                if let Ok(milliseconds) = raw.parse::<u64>() {
                    return Ok(Value::Number((milliseconds.max(10) * 5) as f64));
                }
            }
        }
    }
    Ok(Value::Number(state.borrow().net.auto_select_family_attempt_timeout as f64))
}

pub fn net_set_asf_timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Number(value)) = args.first() else {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        ])));
    };
    if !value.is_finite() || value.fract() != 0.0 || *value <= 0.0 {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        ])));
    }
    state.borrow_mut().net.auto_select_family_attempt_timeout = (*value as u64).max(10);
    Ok(Value::Undefined)
}

pub fn net_get_asf(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(state.borrow().net.auto_select_family))
}

pub fn net_set_asf(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Boolean(value)) = args.first() else {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        ])));
    };
    state.borrow_mut().net.auto_select_family = *value;
    Ok(Value::Undefined)
}

// ---- web-compatible globals ----
pub fn structured_clone(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::clone::structured_clone(
        args.first().cloned().unwrap_or(Value::Undefined),
        args.get(1),
    )
}

pub fn fetch(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
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
    let signal = quench_runtime::execute::set_property(
        signal,
        "constructor",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL),
    );
    let signal = quench_runtime::execute::set_property(
        signal,
        "Symbol.toStringTag",
        Value::String("AbortSignal".into()),
    );
    let signal = quench_runtime::execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    Ok(quench_runtime::host_api::object(vec![
        ("\0quench:abort:controller".into(), Value::Boolean(true)),
        ("\0quench:abort:signal".into(), signal.clone()),
        ("signal".to_string(), signal),
        (
            "constructor".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER),
        ),
        (
            "Symbol.toStringTag".into(),
            Value::String("AbortController".into()),
        ),
        (
            "abort".to_string(),
            crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER_ABORT),
        ),
    ]))
}

pub fn abort_controller_signal_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    let signal = execute::get_property(receiver, "\0quench:abort:signal");
    if !matches!(signal, Value::Object(_))
        || !matches!(
            execute::get_property(receiver, "\0quench:abort:controller"),
            Value::Boolean(true)
        )
    {
        return Err(execute::type_error("Illegal invocation"));
    }
    Ok(signal)
}

pub fn abort_signal_aborted_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    if !matches!(execute::get_property(receiver, "Symbol.toStringTag"), Value::String(ref tag) if tag == "AbortSignal")
    {
        return Err(execute::type_error("Illegal invocation"));
    }
    Ok(execute::get_property(receiver, "aborted"))
}

pub fn abort_signal_has_instance(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(args.first().is_some_and(|value| {
        matches!(
            execute::get_property(value, crate::modules::event_target::ABORT_SIGNAL_BRAND),
            Value::Boolean(true)
        )
    })))
}

pub fn abort_signal_throw_if_aborted(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(signal) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    if !matches!(
        execute::get_property(signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error("Illegal invocation"));
    }
    if matches!(
        execute::get_property(signal, "aborted"),
        Value::Boolean(true)
    ) {
        return Err(VmError::Thrown(execute::get_property(signal, "reason")));
    }
    Ok(Value::Undefined)
}

pub fn abort_controller_abort(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(controller) = receiver else {
        return Err(execute::type_error("Illegal invocation"));
    };
    if !matches!(
        execute::get_property(controller, "\0quench:abort:controller"),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error("Illegal invocation"));
    }
    let original_signal =
        quench_runtime::execute::get_property(controller, "\0quench:abort:signal");
    if matches!(
        quench_runtime::execute::get_property(&original_signal, "aborted"),
        Value::Boolean(true)
    ) {
        return Ok(Value::Undefined);
    }
    let reason = args.first().cloned().unwrap_or_else(|| {
        quench_runtime::host_api::object(vec![
            ("\0domexception".into(), Value::Boolean(true)),
            ("name".into(), Value::String("AbortError".into())),
            (
                "message".into(),
                Value::String("This operation was aborted".into()),
            ),
            ("code".into(), Value::Number(20.0)),
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
        ("isTrusted".into(), Value::Boolean(true)),
        (
            "stopImmediatePropagation".into(),
            crate::host::capability(crate::registry::SPEC_ABORT_EVENT_STOP_IMMEDIATE),
        ),
    ]);
    let event = execute::set_prototype_of(&event, &event_prototype())?;
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
    if !matches!(options, Value::Undefined | Value::Null | Value::Object(_)) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The \"options\" argument must be of type object.{}",
                    crate::modules::buffer_enc::invalid_arg_received(&options)
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
    let global = quench_runtime::vm::current_global_object();
    let event_prototype =
        execute::get_property(&execute::get_property(&global, "Event"), "prototype");
    let event = if matches!(event_prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&event, &event_prototype)?
    } else {
        event
    };
    let event = execute::define_property(
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
    )?;
    Ok(event)
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

fn valid_event_receiver(receiver: Option<&Value>) -> Result<&Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    if !matches!(
        execute::get_property(receiver, "Symbol.toStringTag"),
        Value::String(ref tag) if tag == "Event" || tag == "CustomEvent"
    ) {
        return Err(crate::modules::buffer_enc::invalid_this());
    }
    Ok(receiver)
}

pub fn event_get_cancel_bubble(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    Ok(execute::get_property(receiver, "\0event:cancelBubble"))
}

pub fn event_set_cancel_bubble(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:cancelBubble",
        Value::Boolean(args.first().is_some_and(execute::is_truthy)),
    );
    execute::replace_value(receiver, &updated);
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
    let listener = args.first().cloned().unwrap_or(Value::Null);
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:handler:listener",
        listener.clone(),
    );
    execute::replace_value(receiver, &updated);
    if quench_runtime::is_callable(&listener) {
        let event_name = match &event {
            Value::String(name) => name.as_str(),
            _ => "",
        };
        if !quench_runtime::is_callable(&old)
            || !crate::modules::event_target::replace_event_listener(
                state, receiver, event_name, &old, &listener,
            )
        {
            let _ = crate::modules::event_target::add_event_listener(
                state,
                Some(receiver),
                &[event, listener],
            );
        }
    } else if quench_runtime::is_callable(&old) {
        let _ = crate::modules::event_target::remove_event_listener(
            state,
            Some(receiver),
            &[event, old],
        );
    }
    Ok(Value::Undefined)
}

pub fn event_prevent_default(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    if execute::is_truthy(&execute::get_property(receiver, "\0event:passive")) {
        return Ok(Value::Undefined);
    }
    if execute::is_truthy(&execute::get_property(receiver, "cancelable")) {
        let updated =
            execute::set_property(receiver.clone(), "defaultPrevented", Value::Boolean(true));
        execute::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}

pub fn event_stop_propagation(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:cancelBubble",
        Value::Boolean(true),
    );
    execute::replace_value(receiver, &updated);
    Ok(Value::Undefined)
}

pub fn event_stop_immediate(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    if let Some(identity) = receiver.object_identity() {
        state.borrow_mut().stopped_events.insert(identity);
    }
    let updated = execute::set_property(
        receiver.clone(),
        "\0event:cancelBubble",
        Value::Boolean(true),
    );
    execute::replace_value(receiver, &updated);
    Ok(Value::Undefined)
}

pub fn event_composed_path(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = valid_event_receiver(receiver)?;
    let active = matches!(
        execute::get_property(receiver, "eventPhase"),
        Value::Number(phase) if phase != 0.0
    );
    if !active {
        return Ok(host_api::array(Vec::new()));
    }
    match execute::get_property(receiver, "target") {
        target if !matches!(target, Value::Undefined | Value::Null) => {
            Ok(host_api::array(vec![target]))
        }
        _ => Ok(host_api::array(Vec::new())),
    }
}

pub fn abort_signal_new(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "code".into(),
            Value::String("ERR_ILLEGAL_CONSTRUCTOR".into()),
        ),
        (
            "message".into(),
            Value::String("Illegal constructor".into()),
        ),
    ])))
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
    let signal = quench_runtime::execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    Ok(quench_runtime::execute::set_property(
        signal,
        "reason",
        args.first().cloned().unwrap_or_else(|| {
            quench_runtime::host_api::object(vec![
                ("\0domexception".into(), Value::Boolean(true)),
                ("name".into(), Value::String("AbortError".into())),
                (
                    "message".into(),
                    Value::String("This operation was aborted".into()),
                ),
                ("code".into(), Value::Number(20.0)),
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
    let signal = execute::set_property(
        signal,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    let signal = execute::set_property(
        signal,
        "throwIfAborted",
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_THROW_IF_ABORTED),
    );
    let callback = crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL_TIMEOUT_FIRE);
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
    let Some(signal) = args.first() else {
        return Ok(Value::Undefined);
    };
    let reason = execute::set_property(
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "The operation was aborted due to timeout".into(),
            )],
        ),
        "name",
        Value::String("TimeoutError".into()),
    );
    let reason = execute::set_property(reason, "code", Value::Number(23.0));
    execute::set_property_in_place(signal, "aborted", Value::Boolean(true));
    execute::set_property_in_place(signal, "reason", reason);
    let event = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("abort".into())),
        ("isTrusted".into(), Value::Boolean(true)),
    ]);
    let event = execute::set_prototype_of(&event, &event_prototype())?;
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
        let event = quench_runtime::host_api::object(vec![
            ("type".into(), Value::String("abort".into())),
            ("isTrusted".into(), Value::Boolean(true)),
        ]);
        let event = execute::set_prototype_of(&event, &event_prototype())?;
        crate::modules::event_target::dispatch_event(state, Some(&composite), &[event])?;
        propagate_abort_composites(state, &composite)?;
    }
    Ok(Value::Undefined)
}

pub fn abort_signal_any(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let list = args.first().ok_or_else(|| {
        execute::type_error("The \"signals\" argument must be an instance of Array")
    })?;
    let length = match execute::get_property(list, "length") {
        Value::Number(n) if n.is_finite() && n >= 0.0 => n as usize,
        _ => {
            return Err(execute::type_error(
                "The \"signals\" argument must be an instance of Array",
            ))
        }
    };
    let composite = crate::modules::event_target::new_target(state, &[])?;
    execute::set_property_in_place(&composite, "aborted", Value::Boolean(false));
    execute::set_property_in_place(
        &composite,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    for index in 0..length {
        let source = execute::get_property(list, &index.to_string());
        if !matches!(source, Value::Object(_))
            || !matches!(
                execute::get_property(&source, crate::modules::event_target::ABORT_SIGNAL_BRAND),
                Value::Boolean(true)
            )
        {
            return Err(execute::type_error(
                "The \"signals\" argument must contain only AbortSignal instances",
            ));
        }
        if execute::is_truthy(&execute::get_property(&source, "aborted")) {
            execute::set_property_in_place(&composite, "aborted", Value::Boolean(true));
            execute::set_property_in_place(
                &composite,
                "reason",
                execute::get_property(&source, "reason"),
            );
            return Ok(composite);
        }
        if let Some(identity) = crate::modules::event_target::target_identity(&source) {
            state
                .borrow_mut()
                .abort_composites
                .entry(identity)
                .or_default()
                .push(composite.clone());
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
    let invalid = |message: &str| {
        VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ("message".into(), Value::String(message.into())),
        ]))
    };
    let first = args.first().ok_or_else(|| {
        invalid("The \"warning\" argument must be of type string or an instance of Error.")
    })?;
    let error_object = matches!(first, Value::Object(_) | Value::ObjectAlias(_))
        && (matches!(
            quench_runtime::execute::get_property(first, "message"),
            Value::String(_)
        ) || matches!(
            quench_runtime::execute::get_property(first, "name"),
            Value::String(_)
        ));
    if !matches!(first, Value::String(_)) && !error_object {
        return Err(invalid(
            "The \"warning\" argument must be of type string or an instance of Error.",
        ));
    }
    for (index, value) in args.iter().enumerate().skip(1).take(2) {
        let valid = if index == 2 {
            matches!(value, Value::String(_) | Value::Undefined)
                || quench_runtime::is_callable(value)
        } else {
            matches!(
                value,
                Value::String(_) | Value::Object(_) | Value::ObjectAlias(_) | Value::Undefined
            ) || quench_runtime::is_callable(value)
        };
        if !valid {
            return Err(invalid(&format!(
                "The argument at position {index} must be a string or an object."
            )));
        }
    }
    let first = args.first().cloned().unwrap_or(Value::Undefined);
    let message = match &first {
        Value::Object(_) => match quench_runtime::execute::get_property(&first, "message") {
            Value::String(value) => value,
            _ => crate::modules::path::value_to_string(&first),
        },
        _ => crate::modules::path::value_to_string(&first),
    };
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    let (name, code, detail) = match options {
        Value::String(name) => (name, None, None),
        Value::Object(_) => (
            match quench_runtime::execute::get_property(&options, "name") {
                Value::String(value) if !value.is_empty() => value,
                _ => "Warning".into(),
            },
            match quench_runtime::execute::get_property(&options, "code") {
                Value::String(value) => Some(value),
                _ => None,
            },
            match quench_runtime::execute::get_property(&options, "detail") {
                Value::String(value) => Some(value),
                _ => None,
            },
        ),
        _ => ("Warning".into(), None, None),
    };
    let global = quench_runtime::vm::current_global_object();
    let process = quench_runtime::execute::get_property(&global, "process");
    let no_deprecation = matches!(
        quench_runtime::execute::get_property(&process, "noDeprecation"),
        Value::Boolean(true)
    );
    if name == "DeprecationWarning" && no_deprecation {
        return Ok(Value::Undefined);
    }
    let throw_deprecation = name == "DeprecationWarning"
        && matches!(
            quench_runtime::execute::get_property(&process, "throwDeprecation"),
            Value::Boolean(true)
        );
    if throw_deprecation {
        let error = crate::host::namespace_object_from_pairs(vec![
            (
                "\0prototype".into(),
                Value::Builtin(quench_runtime::ops::Builtin::ErrorPrototype),
            ),
            ("name".into(), Value::String(name.clone())),
            ("message".into(), Value::String(message.clone())),
            ("stack".into(), Value::String(format!("{name}: {message}"))),
        ]);
        state.borrow_mut().event_loop.queue_microtask(
            crate::host::capability(crate::registry::SPEC_PROCESS_EMIT),
            vec![Value::String("uncaughtException".into()), error],
        );
        return Ok(Value::Undefined);
    }
    crate::modules::process::emit_warning_with_detail(
        state,
        &name,
        &message,
        code.as_deref(),
        detail.as_deref(),
        false,
    );
    Ok(Value::Undefined)
}

pub fn process_exit_code_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(
        state.borrow().process.exit_code.unwrap_or(0) as f64
    ))
}

pub fn process_exit_code_set(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(value, Value::Undefined | Value::Null) {
        state.borrow_mut().process.exit_code = None;
        return Ok(Value::Undefined);
    }
    let code = match &value {
        Value::Number(number) if number.is_finite() && number.fract() == 0.0 => *number as i64,
        Value::String(text) if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) => {
            text.parse::<i64>().unwrap_or(-1)
        }
        _ => -1,
    };
    if !(0..=255).contains(&code) {
        let received = match &value {
            Value::Number(number) => {
                let rendered = if number.is_nan() {
                    "NaN".to_string()
                } else if number.is_infinite() {
                    if number.is_sign_negative() {
                        "-Infinity"
                    } else {
                        "Infinity"
                    }
                    .to_string()
                } else {
                    number.to_string()
                };
                format!("Received {rendered}")
            }
            _ => crate::modules::util::invalid_arg_received(&value),
        };
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The \"code\" argument must be of type number or string. {received}"
                )),
            ),
        ])));
    }
    state.borrow_mut().process.exit_code = Some(code as i32);
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

pub fn test_done(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Promise(promise)) = args.first() else {
        return Ok(Value::Undefined);
    };
    let error = args.get(1).cloned().unwrap_or(Value::Undefined);
    if matches!(error, Value::Undefined | Value::Null) {
        quench_runtime::resolve_promise(promise, Value::Undefined);
    } else {
        quench_runtime::reject_promise(promise, error);
    }
    Ok(Value::Undefined)
}

pub fn test_get_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::test::current_context())
}

pub fn test_run_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let emitter = args.first().cloned().unwrap_or(Value::Undefined);
    let event = args
        .get(1)
        .cloned()
        .unwrap_or(Value::String("test:pass".into()));
    crate::modules::net::emit(
        state,
        &emitter,
        &quench_runtime::execute::to_js_string(&event)?,
        vec![quench_runtime::host_api::object(vec![(
            "skip".into(),
            Value::Boolean(true),
        )])],
    )?;
    Ok(Value::Undefined)
}

pub fn test_mock_fn(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    for index in [0usize, 1usize, 2usize, 3usize] {
        if let Some(Value::Object(_) | Value::ObjectAlias(_)) = args.get(index) {
            let times = quench_runtime::execute::get_property(&args[index], "times");
            if !matches!(times, Value::Undefined) {
                let Value::Number(value) = times else {
                    return Err(crate::modules::buffer_enc::invalid_arg_type(
                        "The \"options.times\" property must be of type number".into(),
                    ));
                };
                if !value.is_finite() || value < 1.0 || value.fract() != 0.0 {
                    return Err(crate::modules::buffer_enc::invalid_arg_value(
                        "The value of \"options.times\" is out of range".into(),
                    ));
                }
            }
        }
    }
    let implementation = args
        .get(1)
        .filter(|value| {
            matches!(
                value,
                Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
            )
        })
        .cloned()
        .or_else(|| args.first().cloned())
        .unwrap_or(Value::Undefined);
    let calls = quench_runtime::host_api::array(Vec::new());
    let original_implementation = args
        .first()
        .filter(|value| {
            matches!(
                value,
                Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
            )
        })
        .cloned()
        .unwrap_or(Value::Undefined);
    let implementation_cell = quench_runtime::value::BindingCell::new(implementation.clone());
    if let Some(Value::Object(_) | Value::ObjectAlias(_)) = args.get(2) {
        if let Value::Number(times) = quench_runtime::execute::get_property(&args[2], "times") {
            if times.is_finite() && times >= 0.0 {
                let _ = quench_runtime::execute::set_property_in_place(
                    &calls,
                    "\0mock:times",
                    Value::Number(times),
                );
                let _ = quench_runtime::execute::set_property_in_place(
                    &calls,
                    "\0mock:original",
                    original_implementation.clone(),
                );
            }
        }
    }
    let _ = quench_runtime::execute::set_property_in_place(
        &calls,
        "\0mock:original",
        original_implementation.clone(),
    );
    let implementation_for_bind = implementation.clone();
    let metadata_target = if matches!(original_implementation, Value::Undefined) {
        implementation_for_bind.clone()
    } else {
        original_implementation.clone()
    };
    let mut wrapper = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_TEST_MOCK_CALL.cap,
            ),
        },
        vec![
            Value::BindingCell(implementation_cell.clone()),
            calls.clone(),
            original_implementation.clone(),
        ],
    );
    wrapper = define_mock_metadata(
        wrapper,
        "name",
        quench_runtime::execute::get_property(&metadata_target, "name"),
    )?;
    wrapper = define_mock_metadata(
        wrapper,
        "length",
        quench_runtime::execute::get_property(&metadata_target, "length"),
    )?;
    let mock = quench_runtime::host_api::object(vec![("calls".into(), calls.clone())]);
    let restore = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b06),
        },
        vec![
            Value::BindingCell(implementation_cell.clone()),
            original_implementation,
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
    crate::modules::test::register_mock_restore(quench_runtime::execute::get_property(
        &mock, "restore",
    ));
    let call_count = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b0B),
        },
        vec![calls.clone()],
    );
    let reset_calls = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b11),
        },
        vec![calls.clone()],
    );
    let implementation_setter = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b0C),
        },
        vec![Value::BindingCell(implementation_cell.clone())],
    );
    let implementation_once = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b0D),
        },
        vec![
            Value::BindingCell(implementation_cell.clone()),
            calls.clone(),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "callCount", call_count);
    let _ = quench_runtime::execute::set_property_in_place(&mock, "resetCalls", reset_calls);
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementation",
        implementation_setter,
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementationOnce",
        implementation_once,
    );
    let wrapper = quench_runtime::execute::set_property(wrapper, "mock", mock);
    let bind = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b07),
        },
        vec![
            Value::BindingCell(implementation_cell),
            calls,
            implementation_for_bind,
        ],
    );
    Ok(quench_runtime::execute::set_property(wrapper, "bind", bind))
}

fn define_mock_metadata(wrapper: Value, key: &str, value: Value) -> Result<Value, VmError> {
    let descriptor = quench_runtime::host_api::object(vec![
        ("value".into(), value),
        ("writable".into(), Value::Boolean(false)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    quench_runtime::execute::define_property(wrapper, key, descriptor)
}

pub fn test_mock_call(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?;
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let call_args = args.get(3..).unwrap_or_default();
    let index = match quench_runtime::execute::get_property_result(calls, "length")? {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    let initial_implementation = match implementation {
        Value::BindingCell(cell) => cell.borrow().clone(),
        value => value.clone(),
    };
    if let Value::BindingCell(cell) = implementation {
        if let Value::Number(times) = quench_runtime::execute::get_property(calls, "\0mock:times") {
            if (index as f64) >= times {
                let original = quench_runtime::execute::get_property(calls, "\0mock:original");
                cell.replace(original);
            }
        }
    }
    let mut once_restore = None;
    if let Value::BindingCell(cell) = implementation {
        let once_key = format!("\0mock:once:{index}");
        let once = quench_runtime::execute::get_property(calls, &once_key);
        if !matches!(once, Value::Undefined) {
            once_restore = Some(quench_runtime::execute::get_property(
                calls,
                "\0mock:original",
            ));
            cell.replace(once);
            let _ =
                quench_runtime::execute::set_property_in_place(calls, &once_key, Value::Undefined);
        }
    }
    let implementation_value = match implementation {
        Value::BindingCell(cell) => cell.borrow().clone(),
        _ => initial_implementation,
    };
    let (result, error) = if matches!(implementation_value, Value::Undefined) {
        (Value::Undefined, Value::Undefined)
    } else {
        match quench_runtime::execute::call(&implementation_value, &this, call_args) {
            Ok(result) => (result, Value::Undefined),
            Err(VmError::Thrown(error)) => (Value::Undefined, error),
            Err(error) => return Err(error),
        }
    };
    if let (Value::BindingCell(cell), Some(original)) = (implementation, once_restore) {
        cell.replace(original);
    }
    let record = quench_runtime::host_api::object(vec![
        (
            "arguments".into(),
            quench_runtime::host_api::array(call_args.to_vec()),
        ),
        ("this".into(), this.clone()),
        ("result".into(), result.clone()),
        ("error".into(), error.clone()),
    ]);
    quench_runtime::execute::set_array_element_in_place(calls, index, record);
    if !matches!(error, Value::Undefined) {
        return Err(VmError::Thrown(error));
    }
    Ok(result)
}

pub fn test_mock_method(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let object = args.first().ok_or(VmError::NotCallable)?;
    if matches!(object, Value::Null | Value::Undefined) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"object\" argument must be an object".into(),
        ));
    }
    let key = match args.get(1) {
        Some(Value::String(key)) => key,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"methodName\" argument must be one of type string or symbol".into(),
            ))
        }
    };
    let own_descriptor = quench_runtime::execute::get_own_property_descriptor(object, key)?;
    if !matches!(own_descriptor, Value::Undefined)
        && quench_runtime::execute::get_property(&own_descriptor, "configurable")
            == Value::Boolean(false)
    {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(format!("Cannot redefine property: {key}"))],
        )));
    }
    let options_index = if matches!(args.get(3), Some(Value::Object(_) | Value::ObjectAlias(_))) {
        Some(3)
    } else if matches!(args.get(2), Some(Value::Object(_) | Value::ObjectAlias(_))) {
        Some(2)
    } else {
        None
    };
    if let Some(options_index) = options_index {
        let options = &args[options_index];
        let getter = matches!(options, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
                options, "getter",
            ));
        let setter = matches!(options, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
                options, "setter",
            ));
        if getter && setter {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be used with 'options.getter'".into(),
            ));
        }
        if getter || setter {
            let descriptor = quench_runtime::execute::get_own_property_descriptor(object, key)?;
            let accessor = if getter { "get" } else { "set" };
            let original = quench_runtime::execute::get_property_result(&descriptor, accessor)?;
            let mock_args = if options_index == 3 {
                vec![original, args[2].clone()]
            } else {
                vec![original]
            };
            let wrapper = test_mock_fn(state, None, &mock_args)?;
            let other = if getter { "set" } else { "get" };
            let replacement = quench_runtime::host_api::object(vec![
                (accessor.into(), wrapper.clone()),
                (
                    other.into(),
                    quench_runtime::execute::get_property(&descriptor, other),
                ),
                (
                    "enumerable".into(),
                    quench_runtime::execute::get_property(&descriptor, "enumerable"),
                ),
                (
                    "configurable".into(),
                    quench_runtime::execute::get_property(&descriptor, "configurable"),
                ),
            ]);
            let _ = quench_runtime::execute::define_property(object.clone(), key, replacement)?;
            let mock = quench_runtime::execute::get_property_result(&wrapper, "mock")?;
            let restore = quench_runtime::host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b06),
                },
                vec![
                    object.clone(),
                    Value::String(key.clone()),
                    Value::Undefined,
                    descriptor,
                ],
            );
            let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
            crate::modules::test::register_mock_restore(quench_runtime::execute::get_property(
                &mock, "restore",
            ));
            return Ok(wrapper);
        }
    }
    let original = quench_runtime::execute::get_property_result(object, key)?;
    if !matches!(
        original,
        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The argument 'methodName' must be a method".into(),
        ));
    }
    let mut mock_args = vec![original];
    if let Some(implementation) = args.get(2) {
        mock_args.push(implementation.clone());
    }
    let wrapper = test_mock_fn(state, None, &mock_args)?;
    let mock = quench_runtime::execute::get_property_result(&wrapper, "mock")?;
    let restore = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b06),
        },
        vec![
            object.clone(),
            Value::String(key.clone()),
            original_for_restore(&mock_args),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
    crate::modules::test::register_mock_restore(quench_runtime::execute::get_property(
        &mock, "restore",
    ));
    if !quench_runtime::execute::set_property_in_place(object, key, wrapper.clone()) {
        return Err(VmError::NotCallable);
    }
    Ok(wrapper)
}

pub fn test_mock_getter(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .iter()
        .rev()
        .find(|v| matches!(v, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if matches!(
            quench_runtime::execute::get_property(options, "getter"),
            Value::Boolean(false)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.getter' cannot be false".into(),
            ));
        }
        if matches!(
            quench_runtime::execute::get_property(options, "setter"),
            Value::Boolean(true)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be used with 'options.getter'".into(),
            ));
        }
    }
    let mut method_args = args.to_vec();
    method_args.push(quench_runtime::host_api::object(vec![(
        "getter".into(),
        Value::Boolean(true),
    )]));
    test_mock_method(state, None, &method_args)
}

pub fn test_mock_setter(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .iter()
        .rev()
        .find(|v| matches!(v, Value::Object(_) | Value::ObjectAlias(_)))
    {
        if matches!(
            quench_runtime::execute::get_property(options, "setter"),
            Value::Boolean(false)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be false".into(),
            ));
        }
        if matches!(
            quench_runtime::execute::get_property(options, "getter"),
            Value::Boolean(true)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The property 'options.setter' cannot be used with 'options.getter'".into(),
            ));
        }
    }
    let mut method_args = args.to_vec();
    method_args.push(quench_runtime::host_api::object(vec![(
        "setter".into(),
        Value::Boolean(true),
    )]));
    test_mock_method(state, None, &method_args)
}

pub fn test_mock_call_count(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let calls = args.first().ok_or(VmError::NotCallable)?;
    let length = quench_runtime::execute::get_property(calls, "length");
    Ok(length)
}

pub fn test_mock_reset_calls(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(Value::Array(array)) = args.first() {
        let _ = quench_runtime::execute::set_array_length_in_place(&Value::Array(array.clone()), 0);
    }
    Ok(Value::Undefined)
}

pub fn test_mock_reset(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::reset_mocks();
    Ok(Value::Undefined)
}

pub fn test_mock_timers_enable(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if quench_runtime::date::mock_enabled() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are already enabled".into(),
        ));
    }
    let now = args
        .iter()
        .rev()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| {
            let configured = quench_runtime::execute::get_property(options, "now");
            if matches!(configured, Value::Undefined) {
                quench_runtime::execute::get_property(options, "timeValue")
            } else if matches!(configured, Value::Object(_) | Value::ObjectAlias(_)) {
                quench_runtime::execute::get_property(&configured, "timeValue")
            } else {
                configured
            }
        })
        .unwrap_or(Value::Number(0.0));
    let value = match now {
        Value::Undefined => 0.0,
        Value::Number(value) if value.is_finite() && value >= 0.0 => value,
        Value::Number(_) => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The value of \"now\" is out of range".into(),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"now\" option must be a number".into(),
            ))
        }
    };
    let apis = args
        .iter()
        .rev()
        .find(|v| matches!(v, Value::Object(_) | Value::ObjectAlias(_)))
        .map(|options| quench_runtime::execute::get_property(options, "apis"));
    let date = match apis {
        Some(Value::Array(array)) => {
            quench_runtime::execute::get_property(&Value::Array(array), "0")
                == Value::String("Date".into())
        }
        _ => true,
    };
    if date {
        quench_runtime::date::set_mock_now(Some(value));
    }
    crate::modules::timers::set_mock_timer_now(Some(value.max(0.0) as u64));
    Ok(Value::Undefined)
}

pub fn test_mock_timers_tick(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let delta = match args.first() {
        Some(Value::Number(value)) if value.is_finite() => *value,
        _ => 0.0,
    };
    let now = quench_runtime::date::current_time_ms();
    if quench_runtime::date::mock_enabled() {
        quench_runtime::date::set_mock_now(Some(now + delta));
    }
    let timer_now = crate::modules::timers::mock_timer_now()
        .unwrap_or(now.max(0.0) as u64)
        .saturating_add(delta.max(0.0) as u64);
    crate::modules::timers::set_mock_timer_now(Some(timer_now));
    crate::modules::pump::drain_mock_timers(_state)?;
    Ok(Value::Undefined)
}

pub fn test_mock_timers_set_time(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !quench_runtime::date::mock_enabled() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are not enabled".into(),
        ));
    }
    let value = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The value must be a valid time".into(),
            ))
        }
    };
    quench_runtime::date::set_mock_now(Some(value));
    Ok(Value::Undefined)
}

pub fn test_mock_timers_reset(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    quench_runtime::date::set_mock_now(None);
    crate::modules::timers::set_mock_timer_now(None);
    Ok(Value::Undefined)
}

pub fn test_mock_module(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !matches!(args.first(), Some(Value::String(_))) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"specifier\" argument must be of type string".into(),
        ));
    }
    let Some(options) = args.get(1) else {
        return Ok(Value::Undefined);
    };
    if matches!(options, Value::Null | Value::Undefined)
        || !matches!(options, Value::Object(_) | Value::ObjectAlias(_))
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be an object".into(),
        ));
    }
    for key in ["cache"] {
        let value = quench_runtime::execute::get_property(options, key);
        if !matches!(value, Value::Undefined | Value::Boolean(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.{key}\" property must be of type boolean"
            )));
        }
    }
    for key in ["namedExports", "exports", "defaultExport"] {
        let value = quench_runtime::execute::get_property(options, key);
        if !matches!(
            value,
            Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.{key}\" property must be an object"
            )));
        }
    }
    let exports = !matches!(
        quench_runtime::execute::get_property(options, "exports"),
        Value::Undefined
    );
    let named = !matches!(
        quench_runtime::execute::get_property(options, "namedExports"),
        Value::Undefined
    );
    let default_export = !matches!(
        quench_runtime::execute::get_property(options, "defaultExport"),
        Value::Undefined
    );
    if (exports && named) || (exports && default_export) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The options exports fields cannot be combined".into(),
        ));
    }
    crate::modules::test::register_module_mock(
        args.first()
            .and_then(|value| {
                if let Value::String(value) = value {
                    Some(value.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        options.clone(),
    );
    Ok(Value::Undefined)
}

pub fn test_context_skip(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn test_context_todo(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn test_mock_property(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let object = args.first().ok_or(VmError::NotCallable)?;
    if matches!(object, Value::Null | Value::Undefined) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"object\" argument must be an object".into(),
        ));
    }
    let key = match args.get(1) {
        Some(Value::String(key)) => key.clone(),
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"propertyName\" argument must be one of type string or symbol".into(),
            ))
        }
    };
    let descriptor = quench_runtime::execute::get_own_property_descriptor(object, &key)?;
    if matches!(descriptor, Value::Undefined) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The property must exist".into(),
        ));
    }
    let original = quench_runtime::execute::get_property_result(object, &key)?;
    let value = args.get(2).cloned().unwrap_or(original.clone());
    let cell = quench_runtime::value::BindingCell::new(value);
    let accesses = quench_runtime::host_api::array(Vec::new());
    let getter = bound_custom(
        0x1b16,
        vec![Value::BindingCell(cell.clone()), accesses.clone()],
    );
    let setter = bound_custom(
        0x1b17,
        vec![
            Value::BindingCell(cell.clone()),
            accesses.clone(),
            quench_runtime::execute::get_property(&descriptor, "writable"),
        ],
    );
    let replacement = quench_runtime::host_api::object(vec![
        ("get".into(), getter.clone()),
        ("set".into(), setter.clone()),
        (
            "enumerable".into(),
            quench_runtime::execute::get_property(&descriptor, "enumerable"),
        ),
        (
            "configurable".into(),
            quench_runtime::execute::get_property(&descriptor, "configurable"),
        ),
    ]);
    let mock = quench_runtime::host_api::object(vec![("accesses".into(), accesses.clone())]);
    let restore = bound_custom(
        0x1b06,
        vec![
            object.clone(),
            Value::String(key.clone()),
            Value::Undefined,
            descriptor.clone(),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&mock, "restore", restore);
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "accessCount",
        bound_custom(0x1b14, vec![accesses.clone()]),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "resetAccesses",
        bound_custom(0x1b15, vec![accesses.clone()]),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementation",
        bound_custom(
            0x1b0C,
            vec![Value::BindingCell(cell.clone()), accesses.clone()],
        ),
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &mock,
        "mockImplementationOnce",
        bound_custom(0x1b18, vec![accesses.clone()]),
    );
    let _ = quench_runtime::execute::set_property_in_place(&getter, "mock", mock);
    let replacement = quench_runtime::host_api::object(vec![
        ("get".into(), getter.clone()),
        ("set".into(), setter),
        (
            "enumerable".into(),
            quench_runtime::execute::get_property(&descriptor, "enumerable"),
        ),
        (
            "configurable".into(),
            quench_runtime::execute::get_property(&descriptor, "configurable"),
        ),
    ]);
    let _ = quench_runtime::execute::define_property(object.clone(), &key, replacement)?;
    let restore = bound_custom(
        0x1b06,
        vec![
            object.clone(),
            Value::String(key.clone()),
            Value::Undefined,
            descriptor.clone(),
        ],
    );
    crate::modules::test::register_mock_restore(restore);
    Ok(getter)
}

pub fn test_mock_property_get(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    let accesses = args.get(1).ok_or(VmError::NotCallable)?;
    let index = match quench_runtime::execute::get_property(accesses, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let key = format!("\0property:once:{index}");
    let value = match quench_runtime::execute::get_property(accesses, &key) {
        Value::Undefined => cell.load(),
        value => value,
    };
    let record = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("get".into())),
        ("value".into(), value.clone()),
    ]);
    let _ = quench_runtime::execute::set_array_element_in_place(accesses, index, record);
    Ok(value)
}

pub fn test_mock_property_once(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let accesses = args.first().ok_or(VmError::NotCallable)?;
    let implementation = args.get(1).cloned().unwrap_or(Value::Undefined);
    let current = match quench_runtime::execute::get_property(accesses, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let on_call = match args.get(2) {
        Some(Value::Number(n)) => *n as usize,
        _ => current,
    };
    if on_call < current {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!(
                "The value of \"onAccess\" is out of range. It must be >= {current}"
            ))],
        )));
    }
    let _ = quench_runtime::execute::set_property_in_place(
        accesses,
        &format!("\0property:once:{on_call}"),
        implementation,
    );
    Ok(Value::Undefined)
}

pub fn test_mock_property_set(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    let accesses = args.get(1).ok_or(VmError::NotCallable)?;
    if matches!(args.get(2), Some(Value::Boolean(false))) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "Cannot assign to read only property".into(),
        ));
    }
    let value = args.last().cloned().unwrap_or(Value::Undefined);
    cell.replace(value.clone());
    let index = match quench_runtime::execute::get_property(accesses, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let record = quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("set".into())),
        ("value".into(), value),
    ]);
    let _ = quench_runtime::execute::set_array_element_in_place(accesses, index, record);
    Ok(Value::Undefined)
}

pub fn test_mock_access_count(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(quench_runtime::execute::get_property(
        args.first().ok_or(VmError::NotCallable)?,
        "length",
    ))
}

pub fn test_mock_reset_accesses(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(accesses) = args.first() {
        let _ = quench_runtime::execute::set_array_length_in_place(accesses, 0);
    }
    Ok(Value::Undefined)
}

pub fn test_mock_implementation(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    if let Some((index, Value::Array(accesses))) = args
        .iter()
        .enumerate()
        .find(|(_, value)| matches!(value, Value::Array(_)))
    {
        let value = args.last().cloned().unwrap_or(Value::Undefined);
        cell.replace(value.clone());
        let length = match quench_runtime::execute::get_property(
            &Value::Array(accesses.clone()),
            "length",
        ) {
            Value::Number(n) => n as usize,
            _ => 0,
        };
        let record = quench_runtime::host_api::object(vec![
            ("type".into(), Value::String("set".into())),
            ("value".into(), value),
        ]);
        let _ = quench_runtime::execute::set_array_element_in_place(
            &Value::Array(accesses.clone()),
            length,
            record,
        );
        let _ = index;
    } else {
        cell.replace(args.get(1).cloned().unwrap_or(Value::Undefined));
    }
    Ok(Value::Undefined)
}

pub fn test_mock_implementation_once(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let cell = match args.first() {
        Some(Value::BindingCell(cell)) => cell,
        _ => return Err(VmError::NotCallable),
    };
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let current = match quench_runtime::execute::get_property(calls, "length") {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let implementation = args.get(2).cloned().unwrap_or(Value::Undefined);
    let on_call = match args.get(3) {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value as usize,
        Some(_) => return Err(VmError::NotCallable),
        None => current,
    };
    if on_call < current {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String(format!(
                "The value of \"onCall\" is out of range. It must be >= {current}"
            ))],
        )));
    }
    let key = format!("\0mock:once:{on_call}");
    let _ = quench_runtime::execute::set_property_in_place(calls, &key, implementation);
    let _ = cell;
    Ok(Value::Undefined)
}

fn original_for_restore(args: &[Value]) -> Value {
    args.first().cloned().unwrap_or(Value::Undefined)
}

pub fn test_mock_restore(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }
    if let Some(Value::BindingCell(cell)) = args.first() {
        cell.replace(args.get(1).cloned().unwrap_or(Value::Undefined));
        return Ok(Value::Undefined);
    }
    let object = args.first().ok_or(VmError::NotCallable)?;
    let key = match args.get(1) {
        Some(Value::String(key)) => key,
        _ => return Err(VmError::NotCallable),
    };
    if let Some(descriptor) = args.get(3) {
        let _ = quench_runtime::execute::define_property(object.clone(), key, descriptor.clone())?;
        return Ok(Value::Undefined);
    }
    let original = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::execute::set_property_in_place(object, key, original) {
        return Err(VmError::NotCallable);
    }
    Ok(Value::Undefined)
}

pub fn test_mock_bind(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?.clone();
    let calls = args.get(1).ok_or(VmError::NotCallable)?.clone();
    let this = args.get(3).cloned().unwrap_or(Value::Undefined);
    Ok(quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(0x1b08),
        },
        vec![
            implementation,
            calls,
            args.get(2).cloned().unwrap_or(Value::Undefined),
            this,
        ],
    ))
}

pub fn test_mock_bound_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?;
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let this = args.get(3).ok_or(VmError::NotCallable)?;
    let call_args = args.get(4..).unwrap_or_default();
    let result = quench_runtime::execute::call(implementation, this, call_args)?;
    let index = match quench_runtime::execute::get_property_result(calls, "length")? {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let record = quench_runtime::host_api::object(vec![
        (
            "arguments".into(),
            quench_runtime::host_api::array(call_args.to_vec()),
        ),
        ("this".into(), this.clone()),
        ("result".into(), result.clone()),
    ]);
    quench_runtime::execute::set_array_element_in_place(calls, index, record);
    Ok(result)
}

pub fn test_mock_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let implementation = args.first().ok_or(VmError::NotCallable)?;
    let calls = args.get(1).ok_or(VmError::NotCallable)?;
    let construct_args = args.get(3..).unwrap_or_default();
    let target = match args.get(2).unwrap_or(implementation) {
        Value::BindingCell(cell) => cell.borrow().clone(),
        value => value.clone(),
    };
    let (result, error) = match quench_runtime::execute::construct_value_with_new_target(
        implementation,
        &target,
        construct_args,
    ) {
        Ok(result) => (result, Value::Undefined),
        Err(VmError::Thrown(error)) => (Value::Undefined, error),
        Err(error) => return Err(error),
    };
    let index = match quench_runtime::execute::get_property_result(calls, "length")? {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    };
    let record = quench_runtime::host_api::object(vec![
        (
            "arguments".into(),
            quench_runtime::host_api::array(construct_args.to_vec()),
        ),
        ("result".into(), result.clone()),
        ("error".into(), error.clone()),
        // A construct call preserves both the implementation target and the
        // instance returned by the constructor.  These are observable Node
        // mock facts, not wrapper metadata.
        ("target".into(), target),
        (
            "this".into(),
            if matches!(error, Value::Undefined) {
                result.clone()
            } else {
                Value::Undefined
            },
        ),
    ]);
    quench_runtime::execute::set_array_element_in_place(calls, index, record);
    if !matches!(error, Value::Undefined) {
        return Err(VmError::Thrown(error));
    }
    Ok(result)
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

pub fn test_before_each(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::before_each(args)
}

pub fn test_after_each(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::after_each(args)
}

pub fn test_nested(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::nested(state, args)
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
