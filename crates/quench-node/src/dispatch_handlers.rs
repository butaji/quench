//! Per-domain handler trampolines. Each trampoline adapts a
//! module-level function into the canonical `CallHandler`.
//! The handlers table is the single canonical place where the
//! capability id resolves to a Rust function.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;
use quench_runtime::{execute, host_api};

use crate::host::HostState;

pub type CallHandler =
    fn(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>;
pub type ConstructHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;

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
        .and_then(|options| match options {
            Value::Null => Some(usize::MAX / 2),
            Value::Number(value) if value.is_finite() && *value >= 0.0 => {
                Some(value.floor() as usize + 1)
            }
            _ => None,
        });
    let show_hidden = matches!(args.get(1), Some(Value::Boolean(true)))
        || args.get(1).is_some_and(|options| {
            matches!(
                execute::get_property(options, "showHidden"),
                Value::Boolean(true)
            )
        });
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
    if let Some(name) = timer_promise_alias(&original) {
        let timers = crate::modules::require::require(
            state,
            &[Value::String("timers/promises".to_string())],
        );
        if let Ok(timers) = timers {
            return Ok(execute::get_property(&timers, name));
        }
    }
    Ok(bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALL.cap,
        vec![original],
    ))
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
    _: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    let callback = bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALLBACK.cap,
        vec![Value::Promise(Rc::clone(&promise))],
    );
    let mut call_args = args.get(1..).unwrap_or_default().to_vec();
    call_args.push(callback);
    match quench_runtime::vm::call_value(original, &Value::Undefined, &call_args) {
        Ok(_) => {}
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
    let error = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !matches!(error, Value::Undefined | Value::Null) {
        quench_runtime::reject_promise(&promise, error);
    } else {
        let values = args.get(2..).unwrap_or_default();
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
    Ok(Value::object(Vec::new()))
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

pub fn internal_binding(
    _state: &Rc<RefCell<HostState>>,
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
    if name == "util" {
        return Ok(crate::host::namespace_object_from_pairs(vec![(
            "arrayBufferViewHasBuffer".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_VIEW_HAS_BUFFER),
        )]));
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
    crate::modules::process::emit_warning(
        state,
        "DeprecationWarning",
        "Buffer() is deprecated due to security and usability issues. Please use the Buffer.alloc(), Buffer.allocUnsafe(), or Buffer.from() methods instead.",
        Some("DEP0005"),
        true,
    );
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
    crate::modules::os::homedir(state, args)
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
    crate::modules::event_target::dispatch_event(state, Some(&original_signal), &[event])
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
        options,
        "colors",
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
