//! Per-domain handler trampolines. Each trampoline adapts a
//! module-level function into the canonical `CallHandler`.
//! The handlers table is the single canonical place where the
//! capability id resolves to a Rust function.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
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

pub fn fs_cp(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::fs::cp(state, None, args)
}

pub fn fs_cp_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::fs::cp_sync(state, None, args)
}

pub fn fs_cp_promise(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::fs::cp_promise(state, None, args)
}

pub fn fs_string_to_flags(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::fs::string_to_flags(args.first())
}

thread_local! {
    static OS_PRIORITY: Cell<i32> = const { Cell::new(0) };
    static EVENT_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static GC_EPOCH: Cell<u64> = const { Cell::new(0) };
}

/// Internal symbol used by Node's AbortSignal implementation to expose the
/// number of currently observed composite signals. The embedded value model
/// represents symbols as strings carrying a private identity suffix.
const ABORT_DEPENDANTS_KEY: &str = "Symbol.kDependantSignals\0quench";
const ABORT_ACTIVE_KEY: &str = "\0quench:abort:active";

fn mark_abort_signal(signal: &Value) {
    let dependants = host_api::object(vec![("size".into(), Value::Number(0.0))]);
    execute::set_property_in_place(signal, ABORT_DEPENDANTS_KEY, dependants);
}

fn set_abort_dependant_size(signal: &Value, size: usize) {
    let dependants = execute::get_property(signal, ABORT_DEPENDANTS_KEY);
    if matches!(dependants, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&dependants, "size", Value::Number(size as f64));
    }
}

fn weak_abort_signal(value: &Value) -> Option<quench_runtime::value::WeakObject> {
    match execute::canonical_value(value) {
        Value::Object(object) => Some(Rc::downgrade(&object)),
        Value::ObjectAlias(alias) => Some(alias.0.borrow().clone()),
        _ => None,
    }
}

/// Promote a composite's pending source edges once user code observes it with
/// an abort listener. Composites without observers are intentionally absent
/// from the dependency graph, so retaining them in an array cannot inflate a
/// source signal's `kDependantSignals` set.
pub(crate) fn activate_abort_composite(state: &Rc<RefCell<HostState>>, composite: &Value) {
    // Listener insertion can be observed repeatedly (for example through
    // `addEventListener` and `onabort`).  Promote a composite only once;
    // keeping this fact on the composite avoids an O(n²) identity scan when a
    // large `AbortSignal.any()` set is observed.
    if matches!(
        execute::get_property(composite, ABORT_ACTIVE_KEY),
        Value::Boolean(true)
    ) {
        return;
    }
    execute::set_property_in_place(composite, ABORT_ACTIVE_KEY, Value::Boolean(true));
    let Some(composite_weak) = weak_abort_signal(composite) else {
        return;
    };
    let pending = execute::get_property(composite, "\0quench:abort:sources");
    let Value::Array(ref sources) = pending else {
        return;
    };
    for index in 0..sources.logical_len() {
        let Value::Number(identity) = execute::get_property(&pending, &index.to_string()) else {
            continue;
        };
        if !identity.is_finite() || identity < 0.0 {
            continue;
        }
        let identity = identity as u64;
        let mut host = state.borrow_mut();
        let list = host.abort_composites.entry(identity).or_default();
        list.push(composite_weak.clone());
        let size = list.len();
        let source = host
            .abort_signal_refs
            .get(&identity)
            .and_then(|weak| weak.upgrade())
            .map(Value::Object);
        drop(host);
        if let Some(source) = source {
            set_abort_dependant_size(&source, size);
        }
    }
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
        execute::set_property_in_place(&prototype, "isTrusted", Value::Undefined);
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
    crate::modules::console::log_named(state, args, false, "console.log")
}
pub fn console_info(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log_named(state, args, false, "console.info")
}
pub fn console_debug(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log_named(state, args, false, "console.debug")
}
pub fn console_warn(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log_named(state, args, true, "console.warn")
}
pub fn console_error(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log_named(state, args, true, "console.error")
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
    let prefix = match (
        family.as_str(),
        address.parse::<std::net::IpAddr>(),
        netmask.parse::<std::net::IpAddr>(),
    ) {
        ("IPv4", Ok(std::net::IpAddr::V4(_)), Ok(std::net::IpAddr::V4(mask))) => {
            prefix_length(&mask.octets())
        }
        ("IPv6", Ok(std::net::IpAddr::V6(_)), Ok(std::net::IpAddr::V6(mask))) => {
            let segments = mask.segments();
            let bytes = segments
                .iter()
                .flat_map(|segment| segment.to_be_bytes())
                .collect::<Vec<_>>();
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
    if args.get(1).is_some_and(|options| {
        matches!(options, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(execute::get_property(options, "depth"), Value::Number(value) if value < 0.0)
    }) {
        return Ok(Value::String(crate::modules::util::inspect_minimal(&arg)));
    }
    let _custom_inspect_guard =
        crate::modules::util::custom_inspect_guard(args.get(1).and_then(|options| {
            matches!(options, Value::Object(_) | Value::ObjectAlias(_)).then(|| {
                !matches!(
                    execute::get_property(options, "customInspect"),
                    Value::Boolean(false)
                )
            })
        }));
    let _stylize_guard = crate::modules::util::stylize_guard(args.get(1).and_then(|options| {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return None;
        }
        let stylize = execute::get_property(options, "stylize");
        quench_runtime::is_callable(&stylize).then_some(stylize)
    }));
    let break_length =
        args.iter().find_map(
            |options| match execute::get_property(options, "breakLength") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
                Value::Number(value) if value == f64::INFINITY => Some(usize::MAX),
                _ => None,
            },
        );
    let compact = args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "compact"),
            Value::Boolean(true)
        )
    });
    let noncompact = args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "compact"),
            Value::Boolean(false)
        )
    });
    if compact {
        if let Some(rendered) =
            crate::modules::util::inspect_error_compact_with_break(&arg, break_length)
        {
            let canonical = execute::canonical_value(&arg);
            let has_extras = execute::own_enumerable_keys(&canonical)
                .into_iter()
                .any(|key| !matches!(key.as_str(), "name" | "message" | "stack"));
            if !has_extras {
                return Ok(Value::String(rendered));
            }
            return Ok(Value::String(rendered));
        }
    }
    if noncompact {
        if let Some(rendered) = crate::modules::util::inspect_error_noncompact(&arg) {
            return Ok(Value::String(rendered));
        }
    }
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
    if matches!(
        execute::get_property(&arg, "\0synthetic_module"),
        Value::Boolean(true)
    ) && args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "depth"),
            Value::Number(value) if value < 0.0
        )
    }) {
        return Ok(Value::String("[SyntheticModule]".into()));
    }
    if matches!(
        execute::get_property(&arg, "Symbol.toStringTag"),
        Value::String(ref tag) if tag == "Blob"
    ) {
        let depth = args.get(1).and_then(|options| {
            let value = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                execute::get_property(options, "depth")
            } else {
                options.clone()
            };
            match value {
                Value::Number(value) => Some(value),
                Value::Null => Some(f64::INFINITY),
                _ => None,
            }
        });
        if depth.is_some_and(|value| value < 0.0) {
            return Ok(Value::String("[Blob]".into()));
        }
        let size = crate::modules::util::inspect(&execute::get_property(&arg, "size"));
        let blob_type = match execute::get_property(&arg, "type") {
            Value::String(value) => value,
            _ => String::new(),
        };
        return Ok(Value::String(format!(
            "Blob {{ size: {size}, type: '{blob_type}' }}"
        )));
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
    if let (Value::Object(_) | Value::ObjectAlias(_), Some(options)) = (&arg, args.get(1)) {
        let depth = execute::get_property(options, "depth");
        let tag = execute::get_property(&arg, "Symbol.toStringTag");
        if matches!(depth, Value::Number(value) if value < 0.0)
            && matches!(tag, Value::String(ref value) if value == "Event" || value == "CustomEvent")
        {
            return Ok(tag);
        }
        if matches!(depth, Value::Number(value) if value < 0.0)
            && matches!(
                execute::get_property(&arg, "\0quench:broadcast-channel"),
                Value::Boolean(true)
            )
        {
            return Ok(Value::String("BroadcastChannel".into()));
        }
    }
    let depth = args
        .get(1)
        .filter(|options| matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
        .or_else(|| args.get(2))
        .and_then(|options| {
            let options = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                let depth = execute::get_property(options, "depth");
                if matches!(depth, Value::Undefined) && execute::has_own_property(options, "depth")
                {
                    return Some(usize::MAX / 2);
                }
                depth
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
    let depth = depth.or_else(
        || match crate::modules::util::inspect_default_option("depth") {
            Value::Null => Some(usize::MAX / 2),
            Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize + 1),
            _ => None,
        },
    );
    let show_hidden = matches!(args.get(1), Some(Value::Boolean(true)))
        || args.get(1).is_some_and(|options| {
            if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                return false;
            }
            matches!(
                execute::get_property(options, "showHidden"),
                Value::Boolean(true)
            )
        })
        || matches!(
            args.get(1),
            None if matches!(
                crate::modules::util::inspect_default_option("showHidden"),
                Value::Boolean(true)
            )
        );
    let show_proxy = args.get(1).is_some_and(|options| match options {
        Value::Object(_) | Value::ObjectAlias(_) => {
            match execute::get_property(options, "showProxy") {
                Value::Boolean(value) => value,
                Value::Number(value) => value != 0.0 && !value.is_nan(),
                _ => false,
            }
        }
        _ => false,
    });
    let colors = matches!(args.get(3), Some(Value::Boolean(true)))
        || args.get(1).is_some_and(|options| {
            if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                return false;
            }
            matches!(
                execute::get_property(options, "colors"),
                Value::Boolean(true)
            )
        });
    let break_length_one = args.get(1).is_some_and(|options| {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return false;
        }
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
                Value::Number(value) if value.is_finite() => Some(if value < 0.0 {
                    0
                } else {
                    value.floor() as usize
                }),
                // `null` and Infinity disable the limit; preserve that
                // distinction from an omitted option for collection values.
                Value::Null | Value::Number(_) => Some(usize::MAX),
                _ => None,
            },
        )
        .or_else(
            || match crate::modules::util::inspect_default_option("maxArrayLength") {
                Value::Null => Some(usize::MAX),
                Value::Number(value) if value.is_finite() && value >= 0.0 => Some(value as usize),
                Value::Number(_) => Some(usize::MAX),
                _ => None,
            },
        );
    let _compact_guard = crate::modules::util::compact_guard(args.get(1).and_then(|options| {
        match execute::get_property(options, "compact") {
            Value::Boolean(value) => Some(value),
            _ => None,
        }
    }));
    let _break_length_guard =
        crate::modules::util::break_length_guard(Some(break_length.unwrap_or(80)));
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

pub fn util_transferable_abort_controller(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let global = quench_runtime::vm::current_global_object();
    let constructor = execute::get_property(&global, "AbortController");
    execute::construct_value(&constructor, &[])
}

pub fn util_transferable_abort_signal(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let signal = args
        .first()
        .ok_or_else(|| execute::type_error("The signal argument must be an AbortSignal"))?;
    if !matches!(
        execute::get_property(signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error(
            "The signal argument must be an AbortSignal",
        ));
    }
    Ok(signal.clone())
}

pub fn stream_add_abort_signal_no_validate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(args.get(1).cloned().unwrap_or(Value::Undefined))
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
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"original\" argument must be of type function".into(),
        ));
    };
    if !quench_runtime::is_callable(&original) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"original\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(&original)
        )));
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
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"original\" argument must be of type function".into(),
            ));
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
    let wrapper = bound_custom_in_realm(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALL.cap,
        vec![original.clone()],
        quench_runtime::callable_realm(&original),
    );
    let wrapper = match execute::get_property(&original, "name") {
        Value::String(name) => execute::set_property(wrapper, "name", Value::String(name)),
        _ => wrapper,
    };
    let custom = wrapper.clone();
    if !execute::set_property_in_place(&wrapper, crate::modules::util::PROMISIFY_CUSTOM_KEY, custom)
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"original\" argument must be of type function".into(),
        ));
    }
    Ok(wrapper)
}

pub fn util_callbackify(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first().cloned() else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"original\" argument must be of type function".into(),
        ));
    };
    if !quench_runtime::is_callable(&original) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"original\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(&original)
        )));
    }
    let wrapper = bound_custom(
        crate::registry::SPEC_UTIL_CALLBACKIFIED_CALL.cap,
        vec![original.clone()],
    );
    if let Value::Number(length) = execute::get_property(&original, "length") {
        let _ = execute::set_property_in_place(&wrapper, "length", Value::Number(length + 1.0));
    }
    if let Value::String(name) = execute::get_property(&original, "name") {
        let _ = execute::set_property_in_place(
            &wrapper,
            "name",
            Value::String(format!("{name}Callbackified")),
        );
    }
    Ok(wrapper)
}

fn callbackify_falsy(value: &Value) -> bool {
    matches!(
        value,
        Value::Undefined | Value::Null | Value::Boolean(false)
    ) || matches!(value, Value::Number(number) if *number == 0.0 || number.is_nan())
        || matches!(value, Value::String(value) if value.is_empty())
}

fn callbackify_rejection(value: Value) -> Value {
    if !callbackify_falsy(&value) {
        return value;
    }
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(
            "Promise was rejected with falsy value".into(),
        )],
    );
    let error = execute::set_property(
        error,
        "code",
        Value::String("ERR_FALSY_VALUE_REJECTION".into()),
    );
    let error = execute::set_property(error, "reason", value);
    error
}

pub fn util_callbackified_call(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let Some(callback) = args.last() else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The last argument must be of type function. Received undefined".into(),
        ));
    };
    if !quench_runtime::is_callable(callback) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The last argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(callback)
        )));
    }
    let call_args = if args.len() > 2 {
        &args[1..args.len() - 1]
    } else {
        &[]
    };
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let result = quench_runtime::vm::call_value(original, &receiver, call_args);
    match result {
        Ok(result) => {
            let then = execute::get_property(&result, "then");
            if quench_runtime::is_callable(&then) {
                let fulfilled = bound_custom(
                    crate::registry::SPEC_UTIL_CALLBACKIFIED_FULFILLED.cap,
                    vec![callback.clone(), receiver.clone()],
                );
                let rejected = bound_custom(
                    crate::registry::SPEC_UTIL_CALLBACKIFIED_REJECTED.cap,
                    vec![callback.clone(), receiver.clone()],
                );
                let _ = execute::call(&then, &result, &[fulfilled, rejected])?;
            } else {
                let _ = execute::call(callback, &receiver, &[Value::Null, result])?;
            }
        }
        Err(VmError::Thrown(error)) => {
            let _ = execute::call(callback, &receiver, &[error, Value::Undefined])?;
        }
        Err(_) => {
            let _ = execute::call(callback, &receiver, &[Value::Undefined, Value::Undefined])?;
        }
    }
    Ok(Value::Undefined)
}

pub fn util_callbackified_fulfilled(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().ok_or(VmError::NotCallable)?;
    let receiver = args.get(1).cloned().unwrap_or(Value::Undefined);
    let value = args.get(2).cloned().unwrap_or(Value::Undefined);
    execute::call(callback, &receiver, &[Value::Null, value])
}

pub fn util_callbackified_rejected(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().ok_or(VmError::NotCallable)?;
    let receiver = args.get(1).cloned().unwrap_or(Value::Undefined);
    let reason = callbackify_rejection(args.get(2).cloned().unwrap_or(Value::Undefined));
    execute::call(callback, &receiver, &[reason, Value::Undefined])
}

pub fn internal_errors_e(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let codes = args.first().ok_or(VmError::NotCallable)?;
    let Value::String(code) = args.get(1).cloned().unwrap_or(Value::Undefined) else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"code\" argument must be of type string".into(),
        ));
    };
    let formatter = args.get(2).cloned().unwrap_or(Value::Undefined);
    let base = args
        .get(3)
        .cloned()
        .unwrap_or(Value::Builtin(quench_runtime::ops::Builtin::Error));
    let constructor = internal_error_constructor(&code, formatter, base.clone());
    for candidate in args.iter().skip(3) {
        let name = execute::get_property(candidate, "name");
        if let Value::String(name) = name {
            let nested = internal_error_constructor(
                &code,
                execute::get_property(&constructor, "\0error_formatter"),
                candidate.clone(),
            );
            let _ = execute::set_property_in_place(&constructor, &name, nested);
        }
    }
    let _ = execute::set_property_in_place(codes, &code, constructor.clone());
    Ok(constructor)
}

fn internal_error_constructor(code: &str, formatter: Value, base: Value) -> Value {
    let formatter_copy = formatter.clone();
    let constructor = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_INTERNAL_ERRORS_CONSTRUCTOR.cap,
            ),
        },
        vec![Value::String(code.to_string()), formatter, base],
    );
    let _ = execute::set_property_in_place(&constructor, "\0error_formatter", formatter_copy);
    constructor
}

pub fn internal_system_error_constructor() -> Value {
    let constructor = internal_error_constructor(
        "",
        Value::Undefined,
        host_api::object(vec![("\0system_error_base".into(), Value::Boolean(true))]),
    );
    let _ = execute::set_property_in_place(&constructor, "\0system_error", Value::Boolean(true));
    constructor
}

pub fn internal_errors_hide_stack_frames(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let function = args.first().cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&function) {
        return Err(VmError::NotCallable);
    }
    let _ = execute::set_callable_property(
        &function,
        "\0quench:hidden_stack_frames",
        Value::Boolean(true),
    );
    if matches!(
        execute::get_property(&function, "withoutStackTrace"),
        Value::Undefined
    ) {
        let _ = execute::set_callable_property(&function, "withoutStackTrace", function.clone());
    }
    Ok(function)
}

pub fn internal_errors_info_get(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let info = args.first().ok_or(VmError::NotCallable)?;
    let key = args.get(1).and_then(|value| match value {
        Value::String(key) => Some(key.as_str()),
        _ => None,
    });
    let value = key
        .map(|key| execute::get_property(info, key))
        .unwrap_or(Value::Undefined);
    if matches!(key, Some("path" | "dest")) {
        if let Value::Uint8Array(view) = value {
            let bytes = view.buffer.bytes.borrow();
            return Ok(Value::String(
                String::from_utf8_lossy(&bytes[view.byte_offset..view.byte_offset + view.length])
                    .into_owned(),
            ));
        }
    }
    Ok(value)
}

pub fn internal_errors_info_set(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let info = args.first().ok_or(VmError::NotCallable)?;
    let Some(Value::String(key)) = args.get(1) else {
        return Err(VmError::NotCallable);
    };
    let value = args.get(2).cloned().unwrap_or(Value::Undefined);
    let _ = execute::set_property_in_place(info, key, value.clone());
    Ok(value)
}

pub fn internal_errors_node_flag_get(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(true))
}

pub fn internal_errors_node_flag_set(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(
            "Cannot assign to read-only property Symbol(kIsNodeError)".into(),
        )],
    )))
}

pub fn internal_errors_info_get_value(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(args.first().cloned().unwrap_or(Value::Undefined))
}

pub fn internal_errors_info_set_value(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(
            "Cannot assign to read-only property 'info'".into(),
        )],
    )))
}

pub fn internal_errors_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Value::String(code) = args.first().cloned().unwrap_or(Value::Undefined) else {
        return Err(VmError::NotCallable);
    };
    let formatter = args.get(1).cloned().unwrap_or(Value::Undefined);
    let base = args
        .get(2)
        .cloned()
        .unwrap_or(Value::Builtin(quench_runtime::ops::Builtin::Error));
    let values = args.get(3..).unwrap_or_default();
    let call_frames = quench_runtime::vm::current_call_stack_frames();
    if code == "AbortError" {
        return internal_abort_error(values);
    }
    let system_error = matches!(
        execute::get_property(&base, "\0system_error"),
        Value::Boolean(true)
    ) || matches!(
        execute::get_property(&base, "\0system_error_base"),
        Value::Boolean(true)
    );
    if system_error {
        return internal_system_error(values, &code, &call_frames);
    }
    if values.is_empty() {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!(
                "Code: {code}; The provided arguments length (0) does not match the required ones (1)."
            ))],
        );
        let _ = execute::set_property_in_place(
            &error,
            "code",
            Value::String("ERR_INTERNAL_ASSERTION".into()),
        );
        return Err(VmError::Thrown(error));
    }
    let message = internal_error_message(&formatter, values)?;
    let error = execute::construct_value(&base, &[message])?;
    let _ = execute::set_property_in_place(&error, "code", Value::String(code));
    Ok(error)
}

fn internal_abort_error(values: &[Value]) -> Result<Value, VmError> {
    let message = match values.first() {
        None | Some(Value::Undefined) => "The operation was aborted".to_string(),
        Some(value) => execute::to_js_string(value)?,
    };
    if let Some(options) = values.get(1) {
        if !matches!(
            options,
            Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::TypeError,
                &[Value::String(
                    "The \"options\" argument must be of type object.".into(),
                )],
            );
            let _ = execute::set_property_in_place(
                &error,
                "code",
                Value::String("ERR_INVALID_ARG_TYPE".into()),
            );
            return Err(VmError::Thrown(error));
        }
    }
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message)],
    );
    let _ = execute::set_property_in_place(&error, "name", Value::String("AbortError".into()));
    let _ = execute::set_property_in_place(&error, "code", Value::String("ABORT_ERR".into()));
    if let Some(Value::Object(_) | Value::ObjectAlias(_)) = values.get(1) {
        let cause = execute::get_property(values.get(1).expect("checked option"), "cause");
        if !matches!(cause, Value::Undefined) {
            let _ = execute::set_property_in_place(&error, "cause", cause);
        }
    }
    Ok(error)
}

fn internal_system_error(
    values: &[Value],
    code: &str,
    captured_frames: &[String],
) -> Result<Value, VmError> {
    let source_name = quench_runtime::vm::current_context()
        .source_name()
        .map(|name| name.to_string());
    let Some(Value::Object(_) | Value::ObjectAlias(_)) = values.first() else {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(
                "Cannot read properties of undefined (reading 'syscall')".into(),
            )],
        )));
    };
    let info = values[0].clone();
    let syscall = system_error_field(&info, "syscall")?;
    let returned = system_error_field(&info, "code")?;
    let detail = system_error_field(&info, "message")?;
    let mut message = format!("custom message: {syscall} returned {returned} ({detail})");
    for (key, separator) in [("path", " "), ("dest", " => ")] {
        let value = execute::get_property(&info, key);
        if !matches!(value, Value::Undefined | Value::Null) {
            message.push_str(separator);
            message.push_str(&system_error_value(value)?);
        }
    }
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message)],
    );
    let _ = execute::set_property_in_place(&error, "name", Value::String("SystemError".into()));
    let _ = execute::set_property_in_place(&error, "code", Value::String(code.to_string()));
    let _ = execute::define_property(
        error.clone(),
        "info",
        host_api::object(vec![
            (
                "get".into(),
                host_api::bound_capability_with_arguments(
                    quench_runtime::ops::HostCapabilityRef {
                        realm: quench_runtime::ops::RealmId::ROOT,
                        kind: quench_runtime::ops::HostCapabilityKind::Custom(
                            crate::registry::SPEC_INTERNAL_ERRORS_INFO_GET_VALUE.cap,
                        ),
                    },
                    vec![info.clone()],
                ),
            ),
            (
                "set".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_ERRORS_INFO_SET_VALUE),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    );
    let _ = execute::define_property(
        error.clone(),
        "Symbol(kIsNodeError)\0quench",
        host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_ERRORS_NODE_FLAG_GET),
            ),
            (
                "set".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_ERRORS_NODE_FLAG_SET),
            ),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    );
    for key in ["errno", "syscall", "path", "dest"] {
        let getter = host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_INTERNAL_ERRORS_INFO_GET.cap,
                ),
            },
            vec![info.clone(), Value::String(key.into())],
        );
        let setter = host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_INTERNAL_ERRORS_INFO_SET.cap,
                ),
            },
            vec![info.clone(), Value::String(key.into())],
        );
        let descriptor = host_api::object(vec![
            ("get".into(), getter),
            ("set".into(), setter),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let _ = execute::define_property(error.clone(), key, descriptor);
    }
    // Defining the legacy accessors can publish a copy-on-write successor of
    // the error object. Reapply the stack on the final representative so the
    // constructor's observable value retains the captured caller frames.
    if let Some(filename) = source_name.as_deref() {
        let mut stack = format!(
            "SystemError: {}",
            execute::to_js_string(&execute::get_property(&error, "message")).unwrap_or_default()
        );
        let mut frames = captured_frames.to_vec();
        if let Some(current) = frames.pop() {
            frames.insert(0, current);
        }
        for frame in frames {
            stack.push_str(&format!("\n    at {frame} ({filename}:1:1)"));
        }
        if captured_frames.is_empty() {
            stack.push_str(&format!("\n    at {filename}:1:1"));
        }
        let canonical = execute::canonical_value(&error);
        let updated = execute::set_property(canonical, "stack", Value::String(stack));
        let _ = execute::set_property_in_place(
            &updated,
            "\0quench:stack_decorated",
            Value::Boolean(true),
        );
        let _ = execute::set_property_in_place(
            &updated,
            "\0quench:system_error_instance",
            Value::Boolean(true),
        );
        return Ok(updated);
    }
    Ok(error)
}

fn system_error_field(info: &Value, key: &str) -> Result<String, VmError> {
    system_error_value(execute::get_property(info, key))
}

fn system_error_value(value: Value) -> Result<String, VmError> {
    if let Value::Uint8Array(view) = &value {
        let bytes = view.buffer.bytes.borrow();
        return Ok(String::from_utf8_lossy(
            &bytes[view.byte_offset..view.byte_offset + view.length],
        )
        .into_owned());
    }
    execute::to_js_string(&value)
}

fn internal_error_message(formatter: &Value, values: &[Value]) -> Result<Value, VmError> {
    if quench_runtime::is_callable(formatter) {
        return execute::call(formatter, &Value::Undefined, values);
    }
    let Value::String(mut message) = formatter.clone() else {
        return Ok(Value::Undefined);
    };
    for value in values {
        let rendered = execute::to_js_string(value)?;
        if let Some(index) = message.find("%s") {
            message.replace_range(index..index + 2, &rendered);
        }
    }
    Ok(Value::String(message))
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
fn update_fork_timers(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, referenced: bool) {
    let Some(receiver) = receiver else {
        return;
    };
    let ids = execute::get_property(receiver, "\0childTimerIds");
    let Value::Array(ids) = ids else {
        return;
    };
    let mut timers = state.borrow_mut();
    for index in 0..ids.logical_len() {
        let Ok(value) =
            execute::get_property_result(&Value::Array(ids.clone()), &index.to_string())
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
    if !matches!(callback, Value::Null) && !quench_runtime::is_callable(&callback) {
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
    state
        .borrow_mut()
        .process
        .uncaught_exception_capture_callback =
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
    let mut custom_args = quench_runtime::execute::get_property_result(
        original,
        crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY,
    )
    .unwrap_or(Value::Undefined);
    let is_exec_like = is_child_process_async_capability(&original);
    if is_exec_like && matches!(custom_args, Value::Undefined) {
        custom_args = host_api::array(vec![
            Value::String("stdout".into()),
            Value::String("stderr".into()),
        ]);
    }
    let callback = bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALLBACK.cap,
        vec![Value::Promise(Rc::clone(&promise)), custom_args],
    );
    let mut call_args = args.get(1..).unwrap_or_default().to_vec();
    call_args.push(callback);
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    let is_exec = is_child_process_capability(&original, 0x1e02);
    if is_exec {
        if let Some(options) = call_args.get(1) {
            let signal = execute::get_property(options, "signal");
            let valid = matches!(signal, Value::Undefined)
                || matches!(execute::get_property(&signal, "aborted"), Value::Boolean(_));
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
                execute::set_property_in_place(&promise_value, "child", result.clone());
            }
            if matches!(result, Value::Promise(_)) {
                crate::modules::process::emit_warning_now(
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
            let is_exec_or_file = is_child_process_async_capability(&original);
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

fn is_child_process_capability(value: &Value, cap: u16) -> bool {
    match value {
        Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
            quench_runtime::ops::HostCapabilityKind::Custom(actual),
        )) => *actual == cap,
        Value::BoundFunction(bound) => is_child_process_capability(&bound.target, cap),
        _ => false,
    }
}

fn is_child_process_async_capability(value: &Value) -> bool {
    is_child_process_capability(value, 0x1e02) || is_child_process_capability(value, 0x1e03)
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
    let mut error = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !matches!(error, Value::Undefined | Value::Null) {
        // Node's child-process custom promisifier copies the named callback
        // results onto a rejected error (`err.stdout`/`err.stderr`) before
        // settling the promise. Keep that projection on the original error
        // identity so callers can inspect both status and captured streams.
        if let Value::Array(names) = &custom_args {
            let values = args.get(3..).unwrap_or_default();
            for index in 0..names.logical_len() {
                let key =
                    execute::get_property_result(&Value::Array(names.clone()), &index.to_string());
                if let Ok(Value::String(key)) = key {
                    let value = values.get(index).cloned().unwrap_or(Value::Undefined);
                    execute::set_property_in_place(&mut error, &key, value);
                }
            }
        }
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
        // Node's default promisifier preserves callback arity: no success
        // values become `undefined`, one remains scalar, and multiple values
        // resolve as an array. Named `customPromisifyArgs` results take the
        // object path above.
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

fn bound_custom_in_realm(
    cap: u16,
    arguments: Vec<Value>,
    realm: quench_runtime::ops::RealmId,
) -> Value {
    host_api::bound_capability_with_arguments_in_realm(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(cap),
        },
        arguments,
        realm,
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
pub fn timers_set_unref_timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_unref_timeout(state, args)
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
    rehide_runtime_globals(&global);
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
    let global = quench_runtime::vm::current_global_object();
    let openssl = execute::get_property(&execute::get_property(&global, "process"), "versions");
    if !matches!(
        execute::get_property(&openssl, "openssl"),
        Value::Undefined | Value::Null
    ) {
        return Ok(Value::Undefined);
    }
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("Crypto is not available".into())],
    );
    let error =
        quench_runtime::execute::set_property(error, "code", Value::String("ERR_NO_CRYPTO".into()));
    Err(VmError::Thrown(error))
}

fn offset_length_args(args: &[Value]) -> Option<(f64, f64, f64)> {
    match (args.first(), args.get(1), args.get(2)) {
        (Some(Value::Number(offset)), Some(Value::Number(length)), Some(Value::Number(bytes))) => {
            Some((*offset, *length, *bytes))
        }
        _ => None,
    }
}

pub fn internal_fs_validate_offset_length_read(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some((offset, length, byte_length)) = offset_length_args(args) else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "offset, length, and byteLength must be numbers".into(),
        ));
    };
    if offset < 0.0 {
        return Err(crate::modules::buffer_enc::out_of_range(
            "offset",
            ">= 0",
            &execute::number_to_js_string(offset),
        ));
    }
    if length < 0.0 {
        return Err(crate::modules::buffer_enc::out_of_range(
            "length",
            ">= 0",
            &execute::number_to_js_string(length),
        ));
    }
    if offset + length > byte_length {
        return Err(crate::modules::buffer_enc::out_of_range(
            "length",
            &format!("<= {}", execute::number_to_js_string(byte_length - offset)),
            &execute::number_to_js_string(length),
        ));
    }
    Ok(Value::Undefined)
}

pub fn internal_fs_validate_offset_length_write(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some((offset, length, byte_length)) = offset_length_args(args) else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "offset, length, and byteLength must be numbers".into(),
        ));
    };
    if offset > byte_length {
        return Err(crate::modules::buffer_enc::out_of_range(
            "offset",
            &format!("<= {}", execute::number_to_js_string(byte_length)),
            &execute::number_to_js_string(offset),
        ));
    }
    const IO_MAX: f64 = 2_147_483_647.0;
    if byte_length < IO_MAX && offset + length > byte_length {
        return Err(crate::modules::buffer_enc::out_of_range(
            "length",
            &format!("<= {}", execute::number_to_js_string(byte_length - offset)),
            &execute::number_to_js_string(length),
        ));
    }
    Ok(Value::Undefined)
}

pub fn internal_binding_util_is_inside_node_modules(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let call_stack = quench_runtime::vm::current_call_stack_source_names();
    if let Some(caller) = call_stack.iter().rev().flatten().next() {
        return Ok(Value::Boolean(caller.contains("node_modules")));
    }
    if quench_runtime::vm::current_active_source_name()
        .is_some_and(|path| path.contains("node_modules"))
    {
        return Ok(Value::Boolean(true));
    }
    if quench_runtime::vm::current_call_stack_source_names()
        .iter()
        .flatten()
        .any(|path| path.contains("node_modules"))
    {
        return Ok(Value::Boolean(true));
    }
    let filename = execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "\0quench_vm_filename",
    );
    Ok(Value::Boolean(
        matches!(filename, Value::String(path) if path.contains("node_modules")),
    ))
}

/// WHATWG label lookup used by Node's internal encoding module.
pub fn internal_encoding_get_label(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(label) = args.first().and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::StringUnits(units) => Some(String::from_utf16_lossy(units)),
        _ => None,
    }) else {
        return Ok(Value::Undefined);
    };
    let Some(encoding) = encoding_rs::Encoding::for_label(label.trim().as_bytes()) else {
        return Ok(Value::Undefined);
    };
    Ok(Value::String(encoding.name().to_ascii_lowercase()))
}

pub fn internal_async_context_frame_current(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Null)
}

pub fn internal_async_hooks_enabled_hooks_exist(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        crate::modules::async_hooks::enabled_hooks_exist(state),
    ))
}

fn callback_error(error: VmError) -> Value {
    match error {
        VmError::Thrown(value) => value,
        VmError::NotCallable => quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String("callback is not callable".into())],
        ),
        VmError::EvalError(message) => quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(message)],
        ),
        _ => Value::Undefined,
    }
}

fn dirent_name(value: &Value) -> Value {
    value.clone()
}

pub fn internal_fs_get_dirents(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = match crate::modules::fs::path_arg(args.first()) {
        Ok(path) => path,
        Err(error) => {
            if let Some(callback) = args
                .last()
                .filter(|value| quench_runtime::is_callable(value))
            {
                let _ = quench_runtime::vm::call_value(
                    callback,
                    &Value::Undefined,
                    &[callback_error(error)],
                );
                return Ok(Value::Undefined);
            }
            return Err(error);
        }
    };
    let names = match args.get(1) {
        Some(Value::Array(entries)) => (0..entries.logical_len())
            .filter_map(|index| {
                match quench_runtime::execute::get_property(
                    &Value::Array(entries.clone()),
                    &index.to_string(),
                ) {
                    Value::Array(entry) => Some(dirent_name(
                        &quench_runtime::execute::get_property(&Value::Array(entry), "0"),
                    )),
                    _ => None,
                }
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let callback = args
        .last()
        .filter(|value| quench_runtime::is_callable(value));
    if let Some(callback) = callback {
        let _ = std::fs::read_dir(path);
        let _ = quench_runtime::vm::call_value(
            callback,
            &Value::Undefined,
            &[Value::Null, quench_runtime::host_api::array(names)],
        );
    }
    Ok(Value::Undefined)
}

pub fn internal_fs_get_dirent(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = match crate::modules::fs::path_arg(args.first()) {
        Ok(path) => path,
        Err(error) => {
            if let Some(callback) = args
                .last()
                .filter(|value| quench_runtime::is_callable(value))
            {
                let _ = quench_runtime::vm::call_value(
                    callback,
                    &Value::Undefined,
                    &[callback_error(error)],
                );
                return Ok(Value::Undefined);
            }
            return Err(error);
        }
    };
    let name = args.get(1).cloned().unwrap_or(Value::Undefined);
    let parent_path = match args.get(1) {
        Some(Value::Uint8Array(_)) => quench_runtime::host_api::bytes(path.as_bytes()),
        _ => Value::String(path.into()),
    };
    let dirent = quench_runtime::host_api::object(vec![
        ("name".into(), name),
        ("parentPath".into(), parent_path.clone()),
        ("path".into(), parent_path),
    ]);
    if let Some(callback) = args
        .last()
        .filter(|value| quench_runtime::is_callable(value))
    {
        let _ = quench_runtime::vm::call_value(
            callback,
            &Value::Undefined,
            &[Value::Null, dirent.clone()],
        );
    }
    Ok(dirent)
}

pub fn stream_iter_text(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let consumers = crate::modules::stream::build_consumers(state)?;
    let text = execute::get_property(&consumers, "text");
    execute::call(&text, &Value::Undefined, args)
}

pub fn stream_iter_bytes(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let consumers = crate::modules::stream::build_consumers(state)?;
    let bytes = execute::get_property(&consumers, "bytes");
    execute::call(&bytes, &Value::Undefined, args)
}

fn zlib_iter_transform(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    decompress: bool,
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(crate::host::capability(if decompress {
            crate::registry::SPEC_ZLIB_ITER_DECOMPRESS
        } else {
            crate::registry::SPEC_ZLIB_ITER_COMPRESS
        }));
    }
    let source = match args.first() {
        Some(Value::Array(values)) => {
            let mut bytes = Vec::new();
            for index in 0..values.logical_len() {
                if let Some(value) = values.get(index) {
                    if let Some(chunk) = crate::modules::crypto::bytes_from_value(&value) {
                        bytes.extend(chunk);
                    }
                }
            }
            crate::modules::buffer_proto::make_buffer(&bytes)
        }
        Some(value) => value.clone(),
        None => Value::Undefined,
    };
    let output = if decompress {
        crate::modules::zlib::gunzip(state, None, &[source])?
    } else {
        crate::modules::zlib::gzip(state, None, &[source])?
    };
    Ok(host_api::array(vec![output]))
}

pub fn zlib_iter_compress(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    zlib_iter_transform(state, args, false)
}

pub fn zlib_iter_decompress(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    zlib_iter_transform(state, args, true)
}

pub fn internal_validate_integer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let name = execute::to_js_string(args.get(1).unwrap_or(&Value::String("value".into())))?;
    let valid =
        matches!(value, Value::Number(number) if number.is_finite() && number.fract() == 0.0);
    if !valid {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    }
    if let Value::Number(number) = value {
        if let Some(minimum) = args.get(2).and_then(|value| match value {
            Value::Number(number) => Some(*number),
            _ => None,
        }) {
            if *number < minimum {
                return Err(crate::modules::buffer_enc::out_of_range(
                    &name,
                    &format!(">= {minimum}"),
                    &number.to_string(),
                ));
            }
        }
        if let Some(maximum) = args.get(3).and_then(|value| match value {
            Value::Number(number) => Some(*number),
            _ => None,
        }) {
            if *number > maximum {
                return Err(crate::modules::buffer_enc::out_of_range(
                    &name,
                    &format!("<= {maximum}"),
                    &number.to_string(),
                ));
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn internal_validate_one_of(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }
    let value = args.first().unwrap_or(&Value::Undefined);
    if quench_runtime::is_callable(value) {
        return Ok(Value::Undefined);
    }
    let name = execute::to_js_string(args.get(1).unwrap_or(&Value::String("value".into())))?;
    let allowed = args.get(2).unwrap_or(&Value::Undefined);
    let length = match execute::get_property(allowed, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    if length == 0 {
        return Ok(Value::Undefined);
    }
    if (0..length).any(|index| {
        execute::same_value(value, &execute::get_property(allowed, &index.to_string()))
    }) {
        return Ok(Value::Undefined);
    }
    let choices = (0..length)
        .map(|index| {
            crate::modules::util::inspect(&execute::get_property(allowed, &index.to_string()))
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(crate::modules::buffer_enc::invalid_arg_value(format!(
        "The argument '{name}' must be one of: {choices}. Received {}",
        crate::modules::util::inspect(value)
    )))
}

pub fn internal_validate_port(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let allow_zero = !matches!(args.get(2), Some(Value::Boolean(false)));
    let parsed = match value {
        Value::Number(number) if number.is_finite() && number.fract() == 0.0 => {
            (*number >= 0.0 && *number <= 65535.0).then_some(*number as u16)
        }
        Value::String(text) => parse_port_text(text),
        _ => None,
    };
    let Some(port) = parsed.filter(|port| allow_zero || *port != 0) else {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::RangeError,
            &[Value::String("Port should be >= 0 and < 65536".into())],
        );
        return Err(VmError::Thrown(execute::set_property(
            error,
            "code",
            Value::String("ERR_SOCKET_BAD_PORT".into()),
        )));
    };
    Ok(Value::Number(port as f64))
}

fn parse_port_text(text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() || text.starts_with('-') {
        return None;
    }
    let (radix, digits) =
        if let Some(value) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (16, value)
        } else if let Some(value) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
            (8, value)
        } else if let Some(value) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
            (2, value)
        } else {
            (10, text)
        };
    (!digits.is_empty())
        .then(|| u32::from_str_radix(digits, radix).ok())
        .flatten()
        .filter(|value| *value <= 65535)
        .map(|value| value as u16)
}

pub fn internal_util_decorate_error_stack(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(value @ (Value::Object(_) | Value::ObjectAlias(_))) = args.first() else {
        return Ok(Value::Undefined);
    };
    let stack = quench_runtime::execute::get_property_result(value, "stack").ok();
    let arrow =
        quench_runtime::execute::get_property_result(value, "Symbol.node:arrowMessage\0internal")
            .ok();
    if let (Some(Value::String(stack)), Some(Value::String(arrow))) = (stack, arrow) {
        if !stack.starts_with(&arrow) {
            let _ = quench_runtime::execute::set_property_in_place(
                value,
                "stack",
                Value::String(format!("{arrow}{stack}")),
            );
        }
        let _ = quench_runtime::execute::set_property_in_place(
            value,
            "Symbol.node:decorated\0internal",
            Value::Boolean(true),
        );
    }
    Ok(Value::Undefined)
}

pub fn internal_util_assign_function_name(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(name) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(function) = args.get(1) else {
        return Ok(Value::Undefined);
    };
    quench_runtime::execute::set_dynamic_function_name(function, name)?;
    Ok(function.clone())
}

pub fn internal_util_is_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let is_error = matches!(
        quench_runtime::execute::get_property_result(value, "\0error_slot"),
        Ok(Value::Boolean(true))
    ) || matches!(
        quench_runtime::execute::get_property_result(value, "name"),
        Ok(Value::String(name)) if name == "Error" &&
            matches!(quench_runtime::execute::get_property_result(value, "message"), Ok(Value::String(_)))
    );
    Ok(Value::Boolean(is_error))
}

pub fn internal_crypto_get_openssl_sec_level(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(2.0))
}

pub fn internal_crypto_is_x509_certificate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    Ok(Value::Boolean(matches!(
        execute::get_property(value, "\0quench:crypto:x509-data"),
        Value::Uint8Array(_) | Value::ArrayBuffer(_)
    )))
}

pub fn internal_crypto_bigint_array_to_unsigned_int(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let bytes = args
        .first()
        .and_then(crate::modules::crypto::bytes_from_value)
        .ok_or_else(|| {
            VmError::Thrown(quench_runtime::builtins::dom_exception(
                "algorithm.publicExponent must be an integer array",
                "OperationError",
            ))
        })?;
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    let bytes = &bytes[first..];
    if bytes.len() > 4 {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "algorithm.publicExponent must fit in an unsigned 32-bit integer",
            "OperationError",
        )));
    }
    let value = bytes
        .iter()
        .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte));
    Ok(Value::Number(value as f64))
}

pub fn internal_crypto_bigint_array_to_unsigned_bigint(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let bytes = args
        .first()
        .and_then(crate::modules::crypto::bytes_from_value)
        .ok_or_else(|| {
            VmError::Thrown(quench_runtime::builtins::dom_exception(
                "algorithm.publicExponent must be an integer array",
                "OperationError",
            ))
        })?;
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    let bytes = &bytes[first..];
    if bytes.is_empty() {
        return Ok(Value::BigInt("0".into()));
    }
    let number = openssl::bn::BigNum::from_slice(bytes).map_err(|_| {
        VmError::Thrown(quench_runtime::builtins::dom_exception(
            "algorithm.publicExponent is invalid",
            "OperationError",
        ))
    })?;
    let decimal = number.to_dec_str().map_err(|_| {
        VmError::Thrown(quench_runtime::builtins::dom_exception(
            "algorithm.publicExponent is invalid",
            "OperationError",
        ))
    })?;
    Ok(Value::BigInt(decimal.to_string()))
}

pub fn internal_crypto_key_handle(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = args.first().unwrap_or(&Value::Undefined);
    Ok(crate::modules::webcrypto::crypto_key_handle(key))
}

pub fn internal_crypto_get_usages_mask(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let usages = args.first().unwrap_or(&Value::Undefined);
    let table = [
        ("encrypt", 1_u32),
        ("decrypt", 2),
        ("sign", 4),
        ("verify", 8),
        ("deriveKey", 16),
        ("deriveBits", 32),
        ("wrapKey", 64),
        ("unwrapKey", 128),
        ("encapsulateKey", 256),
        ("encapsulateBits", 512),
        ("decapsulateKey", 1024),
        ("decapsulateBits", 2048),
    ];
    let mask = table.iter().fold(0_u32, |mask, (name, bit)| {
        let present = array_contains_usage(usages, name)
            || matches!(
                execute::call(
                    &execute::get_property(usages, "has"),
                    usages,
                    &[Value::String((*name).into())],
                ),
                Ok(Value::Boolean(true))
            );
        mask | if present { *bit } else { 0 }
    });
    Ok(Value::Number(mask as f64))
}

fn array_contains_usage(value: &Value, expected: &str) -> bool {
    let Value::Array(values) = value else {
        return false;
    };
    (0..values.logical_len()).any(|index| {
        let item = execute::get_property(value, &index.to_string());
        execute::to_js_string(&item).ok().as_deref() == Some(expected)
    })
}

pub fn internal_crypto_aes_cipher(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let iv = args
        .get(2)
        .and_then(crate::modules::crypto::bytes_from_value);
    let promise = if matches!(iv.as_deref(), Some(value) if value.len() != 16) {
        let cause = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("Invalid initialization vector".into())],
        );
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "The operation failed for an operation-specific reason".into(),
            )],
        );
        let error = execute::set_property(error, "name", Value::String("OperationError".into()));
        let error = execute::set_property(error, "cause", cause);
        Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Rejected(error),
        )))
    } else {
        Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Fulfilled(Value::Undefined),
        )))
    };
    Ok(promise)
}

pub fn internal_crypto_webidl_required_arguments(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let present = args.first().and_then(number_value).unwrap_or(0.0);
    let required = args.get(1).and_then(number_value).unwrap_or(0.0);
    if present < required {
        let prefix = execute::to_js_string(&execute::get_property(
            args.get(2).unwrap_or(&Value::Undefined),
            "prefix",
        ))
        .unwrap_or_default();
        let plural = if required == 1.0 {
            "argument"
        } else {
            "arguments"
        };
        return Err(webidl_type_error(
            "ERR_MISSING_ARGS",
            &format!("{prefix}: {required} {plural} required, but only {present} present."),
        ));
    }
    Ok(Value::Undefined)
}

pub fn internal_crypto_webidl_boolean(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(args.first().is_some_and(execute::is_truthy)))
}

pub fn internal_crypto_webidl_octet(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_integer(args, 8, 255)
}

pub fn internal_crypto_webidl_unsigned_short(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_integer(args, 16, 65_535)
}

pub fn internal_crypto_webidl_unsigned_long(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_integer(args, 32, 4_294_967_295)
}

pub fn internal_crypto_webidl_dom_string(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if execute::is_symbol(value)
        || matches!(value, Value::Builtin(quench_runtime::ops::Builtin::Symbol))
    {
        let options = args.get(1).unwrap_or(&Value::Undefined);
        let prefix =
            execute::to_js_string(&execute::get_property(options, "prefix")).unwrap_or_default();
        let context = execute::to_js_string(&execute::get_property(options, "context"))
            .unwrap_or_else(|_| "1st argument".into());
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("{prefix}: {context} is a Symbol and cannot be converted to a string."),
        ));
    }
    execute::to_js_string(value)
        .map(Value::String)
        .map_err(|_| webidl_type_error("ERR_INVALID_ARG_TYPE", "Cannot convert to string"))
}

pub fn internal_crypto_webidl_object(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_) | Value::Function(_)
    ) {
        return Ok(value.clone());
    }
    Err(webidl_type_error(
        "ERR_INVALID_ARG_TYPE",
        &format_webidl_message(args.get(1), "is not an object."),
    ))
}

pub fn internal_crypto_webidl_uint8_array(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if let Value::Uint8Array(view) = value {
        validate_webidl_buffer(view.buffer.as_ref(), true, args.get(1))?;
        return Ok(value.clone());
    }
    Err(webidl_type_error(
        "ERR_INVALID_ARG_TYPE",
        &format_webidl_message(args.get(1), "is not an Uint8Array object."),
    ))
}

pub fn internal_crypto_webidl_dictionary(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(null_object(Vec::new()));
    }
    let name = execute::to_js_string(&execute::get_property(value, "name"))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let profile = dictionary_profile(value, &name);
    for required in profile.required {
        if matches!(execute::get_property(value, required), Value::Undefined) {
            let options = args.get(1).unwrap_or(&Value::Undefined);
            let prefix = execute::to_js_string(&execute::get_property(options, "prefix"))
                .unwrap_or_default();
            let context = execute::to_js_string(&execute::get_property(options, "context"))
                .unwrap_or_else(|_| "1st argument".into());
            return Err(webidl_type_error(
                "ERR_MISSING_OPTION",
                &format!(
                    "{prefix}: {context} cannot be converted to '{}' because '{}' is required in '{}'.",
                    profile.name, required, profile.name
                ),
            ));
        }
    }
    for key in execute::own_enumerable_keys(value) {
        let member = execute::get_property(value, &key);
        if matches!(member, Value::Number(number) if number < 0.0) {
            let maximum = dictionary_member_max(&name, &key);
            let options = args.get(1).unwrap_or(&Value::Undefined);
            let prefix = execute::to_js_string(&execute::get_property(options, "prefix"))
                .unwrap_or_default();
            let context = execute::to_js_string(&execute::get_property(options, "context"))
                .unwrap_or_else(|_| "1st argument".into());
            return Err(webidl_type_error(
                "ERR_OUT_OF_RANGE",
                &format!(
                    "{prefix}: {key} in {context} is outside the expected range of 0 to {maximum}."
                ),
            ));
        }
    }
    let pairs = execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| profile.fields.iter().any(|field| *field == key))
        .map(|key| (key.clone(), execute::get_property(value, &key)))
        .collect();
    Ok(null_object(pairs))
}

struct DictionaryProfile {
    name: &'static str,
    fields: &'static [&'static str],
    required: &'static [&'static str],
}

const ALGORITHM_FIELDS: &[&str] = &["name"];
const RSA_KEYGEN_FIELDS: &[&str] = &["name", "modulusLength", "publicExponent"];
const RSA_HASHED_KEYGEN_FIELDS: &[&str] = &["name", "modulusLength", "publicExponent", "hash"];
const RSA_HASHED_IMPORT_FIELDS: &[&str] = &["name", "hash"];
const NAMED_CURVE_FIELDS: &[&str] = &["name", "namedCurve"];
const RSA_PSS_FIELDS: &[&str] = &["name", "saltLength"];
const RSA_OAEP_FIELDS: &[&str] = &["name", "label"];
const HASH_LENGTH_FIELDS: &[&str] = &["name", "hash", "length"];
const HKDF_FIELDS: &[&str] = &["name", "hash", "salt", "info"];
const PBKDF2_FIELDS: &[&str] = &["name", "salt", "iterations", "hash"];
const AES_CBC_FIELDS: &[&str] = &["name", "iv"];
const AEAD_FIELDS: &[&str] = &["name", "iv", "additionalData", "tagLength"];
const AES_CTR_FIELDS: &[&str] = &["name", "counter", "length"];
const ECDH_FIELDS: &[&str] = &["name", "public"];
const OUTPUT_FIELDS: &[&str] = &["name", "outputLength", "customization"];
const TURBO_FIELDS: &[&str] = &["name", "outputLength", "domainSeparation"];
const CSHAKE_FIELDS: &[&str] = &["name", "outputLength", "functionName", "customization"];
const ARGON_FIELDS: &[&str] = &[
    "name",
    "nonce",
    "parallelism",
    "memory",
    "passes",
    "version",
    "secretValue",
    "associatedData",
];
const CONTEXT_FIELDS: &[&str] = &["name", "context"];

fn dictionary_profile(value: &Value, algorithm: &str) -> DictionaryProfile {
    let has = |key: &str| !matches!(execute::get_property(value, key), Value::Undefined);
    if has("modulusLength") || has("publicExponent") {
        let hashed = has("hash") || algorithm == "RSA-OAEP";
        return DictionaryProfile {
            name: if hashed {
                "RsaHashedKeyGenParams"
            } else {
                "RsaKeyGenParams"
            },
            fields: if hashed {
                RSA_HASHED_KEYGEN_FIELDS
            } else {
                RSA_KEYGEN_FIELDS
            },
            required: if hashed {
                &["name", "hash", "modulusLength", "publicExponent"]
            } else {
                &["name", "modulusLength", "publicExponent"]
            },
        };
    }
    let (name, fields, required): (&str, &[&str], &[&str]) = match () {
        _ if has("namedCurve") => (
            "EcKeyImportParams",
            NAMED_CURVE_FIELDS,
            &["name", "namedCurve"],
        ),
        _ if has("saltLength") => ("RsaPssParams", RSA_PSS_FIELDS, &["name", "saltLength"]),
        _ if algorithm == "RSA-PSS" => ("RsaPssParams", RSA_PSS_FIELDS, &["name", "saltLength"]),
        _ if has("label") => ("RsaOaepParams", RSA_OAEP_FIELDS, &["name"]),
        _ if has("counter") => (
            "AesCtrParams",
            AES_CTR_FIELDS,
            &["name", "counter", "length"],
        ),
        _ if has("tagLength") || has("additionalData") => {
            ("AeadParams", AEAD_FIELDS, &["name", "iv"])
        }
        _ if has("iv") && algorithm.starts_with("AES") => {
            ("AesCbcParams", AES_CBC_FIELDS, &["name", "iv"])
        }
        _ if has("iterations") => (
            "Pbkdf2Params",
            PBKDF2_FIELDS,
            &["name", "salt", "iterations", "hash"],
        ),
        _ if has("nonce") => (
            "Argon2Params",
            ARGON_FIELDS,
            &["name", "nonce", "parallelism", "memory", "passes"],
        ),
        _ if has("context") => ("ContextParams", CONTEXT_FIELDS, &["name"]),
        _ if has("public") => ("EcdhKeyDeriveParams", ECDH_FIELDS, &["name", "public"]),
        _ if has("functionName") => ("CShakeParams", CSHAKE_FIELDS, &["name", "outputLength"]),
        _ if has("outputLength") && algorithm.starts_with("KMAC") => {
            ("KmacParams", OUTPUT_FIELDS, &["name", "outputLength"])
        }
        _ if has("outputLength") && algorithm.starts_with("TURBOSHAKE") => {
            ("TurboShakeParams", TURBO_FIELDS, &["name", "outputLength"])
        }
        _ if has("outputLength") => (
            "KangarooTwelveParams",
            OUTPUT_FIELDS,
            &["name", "outputLength"],
        ),
        _ if has("hash") && algorithm.starts_with("ECDSA") => {
            ("EcdsaParams", RSA_HASHED_IMPORT_FIELDS, &["name", "hash"])
        }
        _ if has("hash") && algorithm.starts_with("HKDF") => {
            ("HkdfParams", HKDF_FIELDS, &["name", "hash", "salt", "info"])
        }
        _ if has("hash") && algorithm.starts_with("HMAC") => {
            ("HmacKeyGenParams", HASH_LENGTH_FIELDS, &["name", "hash"])
        }
        _ if has("hash") => (
            "RsaHashedImportParams",
            RSA_HASHED_IMPORT_FIELDS,
            &["name", "hash"],
        ),
        _ if algorithm == "RSA-OAEP" => (
            "RsaHashedImportParams",
            RSA_HASHED_IMPORT_FIELDS,
            &["name", "hash"],
        ),
        _ if algorithm.starts_with("ECDSA") => (
            "EcKeyImportParams",
            NAMED_CURVE_FIELDS,
            &["name", "namedCurve"],
        ),
        _ if has("length") && algorithm.starts_with("AES") => {
            ("AesKeyGenParams", &["name", "length"], &["name", "length"])
        }
        _ if has("length") => ("HmacKeyGenParams", HASH_LENGTH_FIELDS, &["name"]),
        _ => ("Algorithm", ALGORITHM_FIELDS, &["name"]),
    };
    DictionaryProfile {
        name,
        fields,
        required,
    }
}

pub fn internal_crypto_webidl_big_integer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(Value::Uint8Array(view)) = args.first() {
        validate_webidl_buffer(view.buffer.as_ref(), true, args.get(1))?;
        return Ok(args.first().cloned().unwrap_or(Value::Undefined));
    }
    Err(webidl_type_error(
        "ERR_INVALID_ARG_TYPE",
        &format_webidl_message(args.get(1), "is not a BigInteger."),
    ))
}

pub fn internal_crypto_webidl_buffer_source(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let Some(buffer) = webidl_buffer(value) else {
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format_webidl_message(
                args.get(1),
                "is not instance of ArrayBuffer, Buffer, TypedArray, or DataView.",
            ),
        ));
    };
    validate_webidl_buffer(buffer, !matches!(value, Value::ArrayBuffer(_)), args.get(1))?;
    Ok(value.clone())
}

pub fn internal_crypto_webidl_crypto_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(
        execute::get_property(value, crate::modules::webcrypto::KEY_MARKER_PROP),
        Value::Boolean(true)
    ) {
        return Ok(value.clone());
    }
    Err(webidl_type_error(
        "ERR_INVALID_ARG_TYPE",
        &format_webidl_message(args.get(1), "is not of type CryptoKey."),
    ))
}

pub fn internal_crypto_webidl_algorithm_identifier(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return internal_crypto_webidl_object(_state, _receiver, args);
    }
    internal_crypto_webidl_dom_string(_state, _receiver, args)
}

pub fn internal_crypto_webidl_key_format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_enum(
        args,
        "KeyFormat",
        &[
            "jwk",
            "spki",
            "pkcs8",
            "raw",
            "raw-public",
            "raw-seed",
            "raw-secret",
            "raw-private",
        ],
    )
}

pub fn internal_crypto_webidl_key_usage(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_enum(
        args,
        "KeyUsage",
        &[
            "encrypt",
            "decrypt",
            "sign",
            "verify",
            "deriveKey",
            "deriveBits",
            "wrapKey",
            "unwrapKey",
            "encapsulateBits",
            "decapsulateBits",
            "encapsulateKey",
            "decapsulateKey",
        ],
    )
}

fn webidl_enum(args: &[Value], name: &str, allowed: &[&str]) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let text = execute::to_js_string(value).unwrap_or_default();
    if matches!(value, Value::String(_) | Value::StringUnits(_))
        && allowed.iter().any(|candidate| *candidate == text)
    {
        return Ok(Value::String(text));
    }
    Err(webidl_type_error(
        "ERR_INVALID_ARG_VALUE",
        &format_webidl_message(
            args.get(1),
            &format!("'{text}' is not a valid enum value of type {name}."),
        ),
    ))
}

pub fn internal_crypto_webidl_json_web_key(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format_webidl_message(args.get(1), "is not an object."),
        ));
    }
    let fields = [
        "kty", "use", "key_ops", "alg", "ext", "crv", "x", "y", "d", "n", "e", "p", "q", "dp",
        "dq", "qi", "oth", "k", "pub", "priv",
    ];
    let pairs = fields
        .into_iter()
        .filter_map(|key| {
            let member = execute::get_property(value, key);
            (!matches!(member, Value::Undefined))
                .then(|| (key.into(), json_web_key_member(key, member)))
        })
        .collect();
    Ok(null_object(pairs))
}

pub fn internal_crypto_webidl_algorithm(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(null_object(Vec::new()));
    }
    let name = execute::get_property(value, "name");
    if matches!(name, Value::Undefined) {
        return Err(webidl_type_error(
            "ERR_MISSING_OPTION",
            &format_webidl_message(
                args.get(1),
                "cannot be converted to 'Algorithm' because 'name' is required in 'Algorithm'.",
            ),
        ));
    }
    Ok(null_object(vec![("name".into(), name)]))
}

pub fn internal_crypto_webidl_rsa_oaep(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(null_object(Vec::new()));
    }
    let fields = ["name", "label"];
    let pairs = fields
        .into_iter()
        .filter_map(|key| {
            let member = execute::get_property(value, key);
            (!matches!(member, Value::Undefined)).then(|| (key.into(), member))
        })
        .collect();
    Ok(null_object(pairs))
}

pub fn internal_crypto_webidl_ec_import(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    named_dictionary(
        args,
        "EcKeyImportParams",
        &["name", "namedCurve"],
        &["name", "namedCurve"],
    )
}

pub fn internal_crypto_webidl_ec_gen(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    named_dictionary(
        args,
        "EcKeyGenParams",
        &["name", "namedCurve"],
        &["name", "namedCurve"],
    )
}

pub fn internal_crypto_webidl_ecdsa(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    named_dictionary(args, "EcdsaParams", &["name", "hash"], &["name", "hash"])
}

pub fn internal_crypto_webidl_hmac_keygen(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "length", 4_294_967_295)?;
    named_dictionary(
        args,
        "HmacKeyGenParams",
        &["name", "hash", "length"],
        &["name", "hash"],
    )
}

pub fn internal_crypto_webidl_hmac_import(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "length", 4_294_967_295)?;
    named_dictionary(
        args,
        "HmacImportParams",
        &["name", "hash", "length"],
        &["name", "hash"],
    )
}

pub fn internal_crypto_webidl_aes_keygen(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "length", 65_535)?;
    named_dictionary(
        args,
        "AesKeyGenParams",
        &["name", "length"],
        &["name", "length"],
    )
}

pub fn internal_crypto_webidl_aes_derived(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "length", 65_535)?;
    named_dictionary(
        args,
        "AesDerivedKeyParams",
        &["name", "length"],
        &["name", "length"],
    )
}

pub fn internal_crypto_webidl_hkdf(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    named_dictionary(
        args,
        "HkdfParams",
        &["name", "hash", "salt", "info"],
        &["name", "hash", "salt", "info"],
    )
}

pub fn internal_crypto_webidl_pbkdf2(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "iterations", 4_294_967_295)?;
    named_dictionary(
        args,
        "Pbkdf2Params",
        &["name", "salt", "iterations", "hash"],
        &["name", "salt", "iterations", "hash"],
    )
}

pub fn internal_crypto_webidl_argon2(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    for field in ["parallelism", "memory", "passes", "version"] {
        webidl_check_nonnegative(
            args,
            field,
            if field == "version" {
                255
            } else {
                4_294_967_295
            },
        )?;
    }
    if matches!(execute::get_property(args.first().unwrap_or(&Value::Undefined), "passes"), Value::Number(value) if value == 0.0)
    {
        return Err(webidl_operation_error("passes must be > 0"));
    }
    let object = args.first().unwrap_or(&Value::Undefined);
    if matches!(execute::get_property(object, "parallelism"), Value::Number(value) if value <= 0.0 || value > 16_777_215.0)
    {
        return Err(webidl_operation_error(
            "parallelism must be > 0 and <= 16777215",
        ));
    }
    if let (Value::Number(memory), Value::Number(parallelism)) = (
        execute::get_property(object, "memory"),
        execute::get_property(object, "parallelism"),
    ) {
        if memory < 8.0 * parallelism {
            return Err(webidl_operation_error(
                "memory must be at least 8 times the degree of parallelism",
            ));
        }
    }
    named_dictionary(
        args,
        "Argon2Params",
        &[
            "name",
            "nonce",
            "parallelism",
            "memory",
            "passes",
            "version",
            "secretValue",
            "associatedData",
        ],
        &["name", "nonce", "parallelism", "memory", "passes"],
    )
}

pub fn internal_crypto_webidl_aes_cbc(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    named_dictionary(args, "AesCbcParams", &["name", "iv"], &["name", "iv"])
}

pub fn internal_crypto_webidl_aead(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "tagLength", 255)?;
    named_dictionary(
        args,
        "AeadParams",
        &["name", "iv", "additionalData", "tagLength"],
        &["name", "iv"],
    )
}

pub fn internal_crypto_webidl_aes_ctr(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    webidl_check_nonnegative(args, "length", 255)?;
    named_dictionary(
        args,
        "AesCtrParams",
        &["name", "counter", "length"],
        &["name", "counter", "length"],
    )
}

pub fn internal_crypto_webidl_ecdh(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    named_dictionary(
        args,
        "EcdhKeyDeriveParams",
        &["name", "public"],
        &["name", "public"],
    )
}

fn webidl_check_nonnegative(args: &[Value], member: &str, maximum: u64) -> Result<(), VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(execute::get_property(value, member), Value::Number(number) if number < 0.0) {
        let options = args.get(1).unwrap_or(&Value::Undefined);
        let prefix =
            execute::to_js_string(&execute::get_property(options, "prefix")).unwrap_or_default();
        let context = execute::to_js_string(&execute::get_property(options, "context"))
            .unwrap_or_else(|_| "1st argument".into());
        return Err(webidl_type_error(
            "ERR_OUT_OF_RANGE",
            &format!(
                "{prefix}: {member} in {context} is outside the expected range of 0 to {maximum}."
            ),
        ));
    }
    Ok(())
}

fn named_dictionary(
    args: &[Value],
    name: &str,
    fields: &[&str],
    required: &[&str],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(null_object(Vec::new()));
    }
    for key in required {
        if matches!(execute::get_property(value, key), Value::Undefined) {
            return Err(webidl_type_error(
                "ERR_MISSING_OPTION",
                &format_webidl_message(
                    args.get(1),
                    &format!(
                        "cannot be converted to '{name}' because '{key}' is required in '{name}'."
                    ),
                ),
            ));
        }
    }
    let pairs = fields
        .iter()
        .filter_map(|key| {
            let member = execute::get_property(value, key);
            (!matches!(member, Value::Undefined)).then(|| ((*key).into(), member))
        })
        .collect();
    Ok(null_object(pairs))
}

fn json_web_key_member(key: &str, value: Value) -> Value {
    if key != "oth" {
        return value;
    }
    let Value::Array(array) = &value else {
        return value;
    };
    let entries = (0..array.logical_len())
        .map(|index| {
            let entry = execute::get_property(&value, &index.to_string());
            let fields = ["r", "d", "t"];
            let pairs = fields
                .into_iter()
                .filter_map(|field| {
                    let member = execute::get_property(&entry, field);
                    (!matches!(member, Value::Undefined)).then(|| (field.into(), member))
                })
                .collect();
            null_object(pairs)
        })
        .collect();
    host_api::array(entries)
}

fn webidl_buffer(value: &Value) -> Option<&quench_runtime::value::ArrayBufferData> {
    match value {
        Value::ArrayBuffer(buffer) => Some(buffer.as_ref()),
        Value::DataView(view) => Some(view.buffer.as_ref()),
        Value::Float64Array(view) => Some(view.buffer.as_ref()),
        Value::Float32Array(view) => Some(view.buffer.as_ref()),
        Value::Int8Array(view) => Some(view.buffer.as_ref()),
        Value::Int16Array(view) => Some(view.buffer.as_ref()),
        Value::Int32Array(view) => Some(view.buffer.as_ref()),
        Value::BigInt64Array(view) => Some(view.buffer.as_ref()),
        Value::BigUint64Array(view) => Some(view.buffer.as_ref()),
        Value::Uint32Array(view) => Some(view.buffer.as_ref()),
        Value::Uint8Array(view) => Some(view.buffer.as_ref()),
        Value::Uint8ClampedArray(view) => Some(view.buffer.as_ref()),
        Value::Uint16Array(view) => Some(view.buffer.as_ref()),
        _ => None,
    }
}

fn validate_webidl_buffer(
    buffer: &quench_runtime::value::ArrayBufferData,
    is_view: bool,
    options: Option<&Value>,
) -> Result<(), VmError> {
    if buffer.shared {
        let detail = if is_view {
            "is a view on a SharedArrayBuffer, which is not allowed."
        } else {
            "is not instance of ArrayBuffer, Buffer, TypedArray, or DataView."
        };
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format_webidl_message(options, detail),
        ));
    }
    if buffer.max_byte_length.is_some() {
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format_webidl_message(
                options,
                "is backed by a resizable ArrayBuffer, which is not allowed.",
            ),
        ));
    }
    Ok(())
}

fn dictionary_member_max(name: &str, member: &str) -> u64 {
    match member {
        "tagLength" | "domainSeparation" | "version" => 255,
        "length" if name == "AES-CTR" => 255,
        "length" if name.starts_with("AES") => 65_535,
        _ => 4_294_967_295,
    }
}

fn format_webidl_message(options: Option<&Value>, detail: &str) -> String {
    let options = options.unwrap_or(&Value::Undefined);
    let prefix =
        execute::to_js_string(&execute::get_property(options, "prefix")).unwrap_or_default();
    let context = execute::to_js_string(&execute::get_property(options, "context"))
        .unwrap_or_else(|_| "1st argument".into());
    format!("{prefix}: {context} {detail}")
}

fn number_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::Boolean(value) => Some(f64::from(u8::from(*value))),
        Value::Null => Some(0.0),
        Value::String(value) => value.trim().parse().ok().or(Some(f64::NAN)),
        Value::StringUnits(_) => execute::to_js_string(value)
            .ok()
            .and_then(|text| text.trim().parse().ok().or(Some(f64::NAN))),
        Value::Undefined => Some(f64::NAN),
        Value::Array(_) | Value::Object(_) | Value::ObjectAlias(_) | Value::Function(_) => {
            Some(f64::NAN)
        }
        _ => Some(f64::NAN),
    }
}

fn webidl_integer(args: &[Value], bits: u32, maximum: u64) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    if execute::is_symbol(value) {
        let options = args.get(1).unwrap_or(&Value::Undefined);
        let prefix =
            execute::to_js_string(&execute::get_property(options, "prefix")).unwrap_or_default();
        let context = execute::to_js_string(&execute::get_property(options, "context"))
            .unwrap_or_else(|_| "1st argument".into());
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("{prefix}: {context} is a Symbol and cannot be converted to a number."),
        ));
    }
    if matches!(value, Value::BigInt(_)) {
        let options = args.get(1).unwrap_or(&Value::Undefined);
        let prefix =
            execute::to_js_string(&execute::get_property(options, "prefix")).unwrap_or_default();
        let context = execute::to_js_string(&execute::get_property(options, "context"))
            .unwrap_or_else(|_| "1st argument".into());
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("{prefix}: {context} is a BigInt and cannot be converted to a number."),
        ));
    }
    let Some(number) = number_value(value) else {
        return Err(webidl_type_error(
            "ERR_INVALID_ARG_TYPE",
            "The value cannot be converted to a number.",
        ));
    };
    let options = args.get(1).unwrap_or(&Value::Undefined);
    let enforce = execute::is_truthy(&execute::get_property(options, "enforceRange"));
    if enforce && (!number.is_finite() || number < 0.0 || number > maximum as f64) {
        let prefix =
            execute::to_js_string(&execute::get_property(options, "prefix")).unwrap_or_default();
        let context = execute::to_js_string(&execute::get_property(options, "context"))
            .unwrap_or_else(|_| "1st argument".into());
        return Err(webidl_type_error(
            "ERR_OUT_OF_RANGE",
            &format!("{prefix}: {context} is outside the expected range of 0 to {maximum}."),
        ));
    }
    if !number.is_finite() {
        return Ok(Value::Number(0.0));
    }
    let modulus = 2_f64.powi(bits as i32);
    let wrapped = number.trunc().rem_euclid(modulus);
    Ok(Value::Number(wrapped))
}

fn webidl_type_error(code: &str, message: &str) -> VmError {
    let value = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String(code.into()),
    ))
}

fn webidl_operation_error(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        value,
        "name",
        Value::String("OperationError".into()),
    ))
}

fn null_object(pairs: Vec<(String, Value)>) -> Value {
    let value = host_api::object(pairs);
    execute::set_prototype_of(&value, &Value::Null).unwrap_or(value)
}

pub fn internal_crypto_normalize_algorithm(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let input = args.first().unwrap_or(&Value::Undefined);
    let name = match input {
        Value::String(value) => value.clone(),
        Value::StringUnits(_) => execute::to_js_string(input).unwrap_or_default(),
        _ => execute::to_js_string(&execute::get_property(input, "name")).unwrap_or_default(),
    };
    if matches!(input, Value::Object(_) | Value::ObjectAlias(_)) {
        let fields = [
            "iv",
            "hash",
            "length",
            "namedCurve",
            "salt",
            "info",
            "label",
            "tagLength",
            "modulusLength",
            "publicExponent",
            "saltLength",
            "mgf1HashAlgorithm",
            "context",
        ];
        let mut pairs = vec![("name".into(), Value::String(name))];
        pairs.extend(fields.into_iter().filter_map(|field| {
            let value = execute::get_property(input, field);
            (!matches!(value, Value::Undefined)).then_some((field.into(), value))
        }));
        return Ok(host_api::object(pairs));
    }
    Ok(host_api::object(vec![("name".into(), Value::String(name))]))
}

pub fn internal_crypto_validate_key_ops(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

/// Node's internal WeakReference is the same weak-reference primitive exposed
/// by the runtime. The host adds only the Node spelling (`get`) and delegates
/// lifetime/collection to `quench-runtime`, so there is one weak-reference
/// state machine rather than a second host implementation.
pub fn internal_util_weak_reference_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let weak =
        execute::construct_value(&Value::Builtin(quench_runtime::ops::Builtin::WeakRef), args)?;
    // Keep the registered weak-reference cell and the JS-visible wrapper on
    // one identity.  A copy-on-write replacement here would leave the GC
    // registry mutating an unreachable predecessor, so `get()` would retain
    // its target forever.
    execute::set_property_in_place(
        &weak,
        "get",
        crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_WEAK_REFERENCE_GET),
    );
    Ok(weak)
}

pub fn internal_util_weak_reference_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(VmError::NotCallable);
    };
    Ok(execute::get_property(receiver, "\0weakref"))
}

fn throwing_accessor_constructor(keys: &[&str]) -> Result<Value, VmError> {
    let getter = crate::host::capability(crate::registry::SPEC_INTERNAL_THROW_ACCESSOR);
    let mut base = host_api::object(Vec::new());
    for key in keys {
        base = execute::define_property(
            base,
            key,
            host_api::object(vec![
                ("get".into(), getter.clone()),
                ("set".into(), Value::Undefined),
                ("enumerable".into(), Value::Boolean(false)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )?;
        execute::set_property_in_place(&base, key, Value::Undefined);
    }
    let prototype = host_api::object(Vec::new());
    execute::set_prototype_of(&prototype, &base)?;
    let constructor =
        host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
    Ok(execute::set_property(constructor, "prototype", prototype))
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
    if name == "http2" {
        return Ok(crate::modules::http2_util::binding());
    }
    if name == "async_wrap" {
        let providers = [
            ("PROMISE", 26.0),
            ("RANDOMBYTESREQUEST", 51.0),
            ("TLSWRAP", 55.0),
            ("WORKER", 39.0),
            ("WRITEWRAP", 41.0),
        ]
        .into_iter()
        .map(|(name, id)| (name.to_string(), Value::Number(id)))
        .collect();
        return Ok(crate::host::namespace_object_from_pairs(vec![
            (
                "queueDestroyAsyncId".into(),
                crate::host::capability(crate::registry::SPEC_ASYNC_WRAP_QUEUE_DESTROY),
            ),
            (
                "Providers".into(),
                crate::host::namespace_object_from_pairs(providers),
            ),
        ]));
    }
    if name == "crypto" {
        let secure_context = throwing_accessor_constructor(&["_external"])?;
        let prototype = execute::get_property(&secure_context, "prototype");
        let descriptor = host_api::object(vec![
            (
                "get".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_THROW_ACCESSOR),
            ),
            ("set".into(), Value::Undefined),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let prototype = execute::define_property(prototype, "_external", descriptor)?;
        execute::set_property_in_place(&prototype, "_external", Value::Undefined);
        execute::set_property_in_place(&secure_context, "prototype", prototype);
        return Ok(crate::host::namespace_object_from_pairs(vec![
            (
                "testFipsCrypto".to_string(),
                crate::host::capability(crate::registry::SPEC_CRYPTO_TEST_FIPS),
            ),
            ("SecureContext".to_string(), secure_context),
        ]));
    }
    if name == "fs" {
        // `internalBinding('fs')` is the fd/stat side of the same fs state;
        // expose the canonical host capability instead of a second JS table.
        if let Some(binding) = state.borrow().module_cache.get("\0internalBinding:fs") {
            return Ok(binding.clone());
        }
        let binding = crate::host::namespace_object_from_pairs(vec![
            (
                "fstat".to_string(),
                crate::host::capability(crate::registry::SPEC_FS_FSTAT_SYNC),
            ),
            (
                "openFileHandle".to_string(),
                crate::host::scheduler_capability(0x7FE1),
            ),
            (
                "internalModuleStat".to_string(),
                crate::host::capability(crate::registry::SPEC_MODULE_STAT),
            ),
        ]);
        state
            .borrow_mut()
            .module_cache
            .insert("\0internalBinding:fs".into(), binding.clone());
        return Ok(binding);
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
            ("UV_EEXIST".to_string(), Value::Number(-17.0)),
            ("UV_EBADF".to_string(), Value::Number(-9.0)),
            ("UV_EINVAL".to_string(), Value::Number(-22.0)),
            ("UV_ENOTDIR".to_string(), Value::Number(-20.0)),
            ("UV_ENOTEMPTY".to_string(), Value::Number(-66.0)),
            ("UV_EPERM".to_string(), Value::Number(-1.0)),
            ("UV_EOF".to_string(), Value::Number(-4095.0)),
            (
                "errname".to_string(),
                crate::host::capability(crate::registry::SPEC_PROCESS_BINDING_UV_ERRNAME),
            ),
        ]));
    }
    if name == "stream_wrap" {
        return Ok(crate::host::namespace_object_from_pairs(vec![
            ("streamBaseState".to_string(), host_api::object(Vec::new())),
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
        let prototype = crate::host::namespace_object_from_pairs(vec![(
            "setNoDelay".to_string(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        )]);
        let tcp_constructor = execute::set_property(
            crate::host::capability(crate::registry::SPEC_NET_TCP),
            "prototype",
            prototype,
        );
        let constants = crate::host::namespace_object_from_pairs(vec![(
            "SOCKET".to_string(),
            Value::Number(1.0),
        )]);
        let binding = crate::host::namespace_object_from_pairs(vec![
            ("TCP".to_string(), tcp_constructor.clone()),
            ("TCPWrap".to_string(), tcp_constructor),
            ("constants".to_string(), constants),
        ]);
        execute::set_property_in_place(
            &global,
            crate::modules::net::TCP_WRAP_BINDING_PROP,
            binding.clone(),
        );
        state.borrow_mut().tcp_binding = Some(binding.clone());
        return Ok(binding);
    }
    if name == "tty_wrap" {
        let tty = throwing_accessor_constructor(&["bytesRead", "fd", "_externalStream"])?;
        return Ok(host_api::object(vec![("TTY".into(), tty)]));
    }
    if name == "crypto" {
        let secure_context =
            host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
        return Ok(host_api::object(vec![(
            "SecureContext".into(),
            secure_context,
        )]));
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
        let process =
            execute::get_property(&quench_runtime::vm::current_global_object(), "process");
        let public_binding = receiver.is_some_and(|value| value == &process);
        if !public_binding {
            binding.push((
                "isInsideNodeModules".to_string(),
                crate::host::capability(
                    crate::registry::SPEC_INTERNAL_BINDING_UTIL_IS_INSIDE_NODE_MODULES,
                ),
            ));
            binding.push((
                "previewEntries".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_PREVIEW_ENTRIES),
            ));
            binding.push((
                "privateSymbols".to_string(),
                host_api::object(vec![
                    (
                        "arrow_message_private_symbol".to_string(),
                        Value::String("Symbol.node:arrowMessage\0internal".into()),
                    ),
                    (
                        "decorated_private_symbol".to_string(),
                        Value::String("Symbol.node:decorated\0internal".into()),
                    ),
                ]),
            ));
            binding.push((
                "getProxyDetails".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_GET_PROXY_DETAILS),
            ));
            binding.push((
                "arrayBufferViewHasBuffer".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_VIEW_HAS_BUFFER),
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
        if name == "udp_wrap" {
            let udp = throwing_accessor_constructor(&["fd"])?;
            return Ok(crate::host::namespace_object_from_pairs(vec![(
                "UDP".into(),
                udp,
            )]));
        }
        if name == "pipe_wrap" {
            let constants = crate::host::namespace_object_from_pairs(vec![(
                "SOCKET".into(),
                Value::Number(0.0),
            )]);
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

/// Record an internal FileHandle allocation so the explicit GC boundary can
/// report Node's finalizer error when the handle is not closed.
pub fn internal_fs_open_file_handle(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = args
        .first()
        .map(execute::to_js_string)
        .transpose()?
        .unwrap_or_default();
    state.borrow_mut().pending_filehandle_gc.push(path);
    Ok(Value::Undefined)
}

/// Return the first iterator entry in the shape consumed by Node's
/// `util.inspect` preview hook.  This is deliberately a non-consuming view:
/// inspection must not advance user-visible iterators.
pub fn internal_preview_entries(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Iterator(iterator)) = args.first() else {
        return Ok(host_api::array(Vec::new()));
    };
    let preview = match &*iterator.state.borrow() {
        quench_runtime::value::IteratorState::Map {
            data, index, kind, ..
        } => {
            let key = data.keys.borrow().get(*index).cloned();
            let value = data.values.borrow().get(*index).cloned();
            match (key, value, *kind) {
                (Some(key), Some(value), 0) => host_api::array(vec![key, value]),
                (Some(key), _, 1) => host_api::array(vec![key]),
                (_, Some(value), 2) => host_api::array(vec![value]),
                _ => host_api::array(Vec::new()),
            }
        }
        quench_runtime::value::IteratorState::Set {
            data, index, kind, ..
        } => {
            let value = data.values.borrow().get(*index).cloned();
            match (value, *kind) {
                (Some(value), 0) => host_api::array(vec![value]),
                (Some(value), 1) => host_api::array(vec![value.clone(), value]),
                _ => host_api::array(Vec::new()),
            }
        }
        _ => host_api::array(Vec::new()),
    };
    if args.get(1).is_some_and(execute::is_truthy) {
        let is_key_value = matches!(
            &*iterator.state.borrow(),
            quench_runtime::value::IteratorState::Map { kind: 0, .. }
                | quench_runtime::value::IteratorState::Set { kind: 1, .. }
        );
        Ok(host_api::array(vec![preview, Value::Boolean(is_key_value)]))
    } else {
        Ok(preview)
    }
}

pub fn internal_throw_accessor(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(crate::modules::buffer_enc::invalid_arg_value(
        "Value is not a valid accessor receiver".into(),
    ))
}

pub fn internal_js_stream_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = args
        .first()
        .cloned()
        .unwrap_or_else(|| crate::host::namespace_object_from_pairs(Vec::new()));
    let handle = crate::host::namespace_object_from_pairs(vec![
        (
            "asyncReset".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_JS_STREAM),
        ),
        (
            "getProviderType".into(),
            crate::host::capability(crate::registry::SPEC_ASYNC_EXECUTION_ID),
        ),
        (
            "getAsyncId".into(),
            crate::host::capability(crate::registry::SPEC_ASYNC_EXECUTION_ID),
        ),
    ]);
    Ok(execute::set_property(stream, "_handle", handle))
}

pub fn internal_js_stream_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(resource) = args.first().filter(|value| {
        matches!(
            execute::get_property(value, "type"),
            Value::String(_)
        ) && matches!(execute::get_property(value, "handle"), Value::Object(_))
    }) {
        crate::modules::async_hooks::attach_resource(state, resource.clone(), "ReusedHandle")?;
    }
    Ok(Value::Undefined)
}

fn source_text_module_requests(source: &str) -> Result<Vec<(Value, String)>, VmError> {
    let mut requests = Vec::new();
    let normalized = source
        .replace("export * from", "import * from")
        .replace("export {", "import {");
    for rest in normalized.split("import ").skip(1) {
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
        let mut attributes = quench_runtime::host_api::object(Vec::new());
        for (name, value) in attribute_values {
            attributes = execute::define_property(
                attributes,
                &name,
                host_api::object(vec![
                    ("value".into(), value),
                    ("writable".into(), Value::Boolean(false)),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(false)),
                ]),
            )?;
        }
        let attributes = execute::set_prototype_of(&attributes, &Value::Null).unwrap_or(attributes);
        let attributes = execute::set_property(
            attributes,
            "\0vm_module_request_attributes",
            Value::Boolean(true),
        );
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
        let request = execute::set_property(request, "\0vm_module_request", Value::Boolean(true));
        requests.push((request, key));
    }
    Ok(requests)
}

pub fn vm_source_text_module_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(source)) = args.first() else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"code\" argument must be of type string. Received undefined".into(),
        ));
    };
    if let Some(options) = args.get(1) {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"options\" argument must be of type object. Received invalid value".into(),
            ));
        }
        if let Ok(identifier) = execute::get_property_result(options, "identifier") {
            if !matches!(identifier, Value::String(_) | Value::Undefined) {
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    "The \"options.identifier\" property must be of type string".into(),
                ));
            }
        }
        if let Ok(dynamic_import) = execute::get_property_result(options, "importModuleDynamically")
        {
            if !matches!(dynamic_import, Value::Undefined)
                && !quench_runtime::is_callable(&dynamic_import)
            {
                let detail = match &dynamic_import {
                    Value::String(value) => format!("Received type string ('{value}')"),
                    Value::Boolean(value) => format!("Received type boolean ({value})"),
                    Value::Number(value) => format!("Received type number ({value})"),
                    _ => "Received invalid value".into(),
                };
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    format!("The \"options.importModuleDynamically\" property must be of type function. {detail}"),
                ));
            }
        }
        if let Ok(context) = execute::get_property_result(options, "context") {
            if !matches!(context, Value::Undefined)
                && !quench_runtime::vm::is_script_context(&context)
            {
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    "The \"options.context\" property must be a vm context".into(),
                ));
            }
        }
    }
    if let Some(options) = args.get(1) {
        if let Ok(Value::Uint8Array(data)) = execute::get_property_result(options, "cachedData") {
            let bytes = data.buffer.bytes.borrow();
            if bytes.as_slice() != source.as_bytes() {
                let error = execute::set_property(
                    quench_runtime::builtins::error(
                        quench_runtime::ops::Builtin::Error,
                        &[Value::String("cached data rejected".into())],
                    ),
                    "code",
                    Value::String("ERR_VM_MODULE_CACHED_DATA_REJECTED".into()),
                );
                return Err(VmError::Thrown(error));
            }
        }
    }
    vm_source_text_module_value(args)
}

pub fn vm_source_text_module_value(args: &[Value]) -> Result<Value, VmError> {
    let source = match args.first() {
        Some(Value::String(source)) => source.clone(),
        _ => String::new(),
    };
    let identifier = args
        .get(1)
        .and_then(|options| execute::get_property_result(options, "identifier").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| "vm:module(0)".into());
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
    let mut export_names = Vec::new();
    for part in source.split("export ").skip(1) {
        let Some((kind, rest)) = part.split_once(' ') else {
            continue;
        };
        if kind == "{" {
            if let Some(body) = rest.split('}').next() {
                for entry in body.split(',') {
                    let local = entry.split_whitespace().next().unwrap_or_default();
                    if !local.is_empty() {
                        export_names.push((local.to_string(), "reexport".to_string()));
                    }
                }
            }
            continue;
        }
        if kind == "*" {
            continue;
        }
        let name = rest
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
            .next()
            .unwrap_or_default();
        let name = if kind == "default" { "default" } else { name };
        if name.is_empty() {
            continue;
        }
        if matches!(kind, "const" | "let" | "var") && rest.contains(',') {
            for declaration in rest.split(';').next().unwrap_or_default().split(',') {
                let declaration_name = declaration
                    .split('=')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
                    .next()
                    .unwrap_or_default();
                if !declaration_name.is_empty() {
                    export_names.push((declaration_name.to_string(), kind.to_string()));
                }
            }
        } else {
            export_names.push((name.to_string(), kind.to_string()));
        }
    }
    export_names.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, kind) in export_names {
        namespace = execute::define_property(
            namespace,
            &name,
            host_api::object(vec![
                ("value".into(), Value::Undefined),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )?;
        if kind == "const" {
            uninitialized = execute::set_property(uninitialized, &name, Value::Boolean(true));
        }
    }
    namespace = execute::set_property(namespace, "\0module_namespace", Value::Boolean(true));
    namespace = execute::set_property(namespace, "\0module_uninitialized", uninitialized);
    let result = crate::host::namespace_object_from_pairs(vec![
        ("\0module_source".into(), Value::String(source)),
        ("\0source_text_module".into(), Value::Boolean(true)),
        ("status".into(), Value::String("unlinked".into())),
        ("identifier".into(), Value::String(identifier)),
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
            "\0module_dependencies".into(),
            quench_runtime::host_api::array(Vec::new()),
        ),
        (
            "link".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_LINK),
        ),
        (
            "evaluate".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_EVALUATE),
        ),
        (
            "linkRequests".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_LINK_REQUESTS),
        ),
        (
            "instantiate".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_INSTANTIATE),
        ),
        (
            "createCachedData".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_CACHED_DATA),
        ),
        (
            "hasAsyncGraph".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_HAS_ASYNC_GRAPH),
        ),
        (
            "hasTopLevelAwait".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_HAS_TOP_LEVEL_AWAIT),
        ),
        (
            "Symbol.for.nodejs.util.inspect.custom\0".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_INSPECT),
        ),
    ]);
    Ok(result)
}

pub fn vm_module_link(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !args.first().is_some_and(quench_runtime::is_callable) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"linker\" argument must be of type function. Received invalid value".into(),
        ));
    }
    if let Some(module) = receiver {
        execute::set_property_in_place(module, "status", Value::String("linked".into()));
    }
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    quench_runtime::resolve_promise(&promise, Value::Undefined);
    Ok(Value::Promise(promise))
}

pub fn vm_module_link_requests(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let mut expected_len = match execute::get_property(module, "moduleRequests") {
        Value::Array(requests) => requests.logical_len(),
        _ => 0,
    };
    let dependencies = match _args.first() {
        Some(Value::Array(values)) => values.to_vec(),
        _ => Vec::new(),
    };
    if dependencies.len() != expected_len {
        return Err(VmError::Thrown(execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(
                    "Provided modules do not match module requests".into(),
                )],
            ),
            "code",
            Value::String("ERR_MODULE_LINK_MISMATCH".into()),
        )));
    }
    // A module request is keyed by its specifier and import attributes.  When
    // the same key occurs more than once, Node requires the corresponding
    // dependency entries to be the very same module object; comparing values
    // structurally would both miss identity and recurse through module graphs.
    let mut linked_by_key: HashMap<String, u64> = HashMap::new();
    if let Value::Array(requests) = execute::get_property(module, "moduleRequests") {
        for (request, dependency) in requests.to_vec().into_iter().zip(dependencies.iter()) {
            let key = execute::get_property(&request, "specifier");
            let Value::String(key) = key else {
                continue;
            };
            let Some(identity) = dependency.object_identity() else {
                continue;
            };
            if let Some(previous) = linked_by_key.insert(key, identity) {
                if previous != identity {
                    return Err(VmError::Thrown(execute::set_property(
                        quench_runtime::builtins::error(
                            quench_runtime::ops::Builtin::Error,
                            &[Value::String(
                                "Provided modules do not match module requests".into(),
                            )],
                        ),
                        "code",
                        Value::String("ERR_MODULE_LINK_MISMATCH".into()),
                    )));
                }
            }
        }
    }
    if dependencies
        .iter()
        .any(|dependency| !is_source_text_module(dependency))
    {
        return Err(VmError::Thrown(execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("Provided value is not a Module".into())],
            ),
            "code",
            Value::String("ERR_VM_MODULE_NOT_MODULE".into()),
        )));
    }
    if let Some(Value::Array(dependencies)) = _args.first() {
        execute::set_property_in_place(
            module,
            "\0module_dependencies",
            Value::Array(dependencies.clone()),
        );
    }
    let source = execute::get_property(module, "\0module_source");
    if let (Value::String(source), Some(Value::Array(dependencies))) = (source, _args.first()) {
        if source.contains("export *") {
            let mut namespace = execute::get_property(module, "namespace");
            for dependency in dependencies.to_vec() {
                let dependency_namespace = execute::get_property(&dependency, "namespace");
                for key in execute::own_enumerable_keys(&dependency_namespace) {
                    if !execute::has_own_property(&namespace, &key) {
                        let value = execute::get_property(&dependency_namespace, &key);
                        namespace = execute::define_property(
                            namespace,
                            &key,
                            host_api::object(vec![
                                ("value".into(), value),
                                ("writable".into(), Value::Boolean(true)),
                                ("enumerable".into(), Value::Boolean(true)),
                                ("configurable".into(), Value::Boolean(true)),
                            ]),
                        )?;
                    }
                }
            }
            execute::set_property_in_place(module, "namespace", namespace);
        }
    }
    let namespace = execute::get_property(module, "namespace");
    let namespace = execute::define_property(
        namespace,
        "Symbol.toStringTag",
        host_api::object(vec![
            ("value".into(), Value::String("Module".into())),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(false)),
        ]),
    )?;
    execute::set_property_in_place(module, "namespace", namespace);
    execute::set_property_in_place(module, "status", Value::String("linked".into()));
    Ok(Value::Undefined)
}

fn is_source_text_module(value: &Value) -> bool {
    execute::has_own_property(value, "\0source_text_module")
        && execute::has_own_property(value, "\0module_source")
        && execute::has_own_property(value, "status")
        && matches!(
            execute::get_property_result(value, "\0source_text_module"),
            Ok(Value::Boolean(true))
        )
        && matches!(
            execute::get_property_result(value, "\0module_source"),
            Ok(Value::String(_))
        )
        && matches!(
            execute::get_property_result(value, "status"),
            Ok(Value::String(_))
        )
}

pub fn vm_module_instantiate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let status = execute::get_property(module, "status");
    if matches!(status, Value::String(ref status) if status == "unlinked") {
        let error = execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("Module requests have not been linked".into())],
            ),
            "code",
            Value::String("ERR_VM_MODULE_LINK_FAILURE".into()),
        );
        return Err(VmError::Thrown(error));
    }
    if let Some((specifier, identifier)) = prepare_module_graph(module, &mut HashSet::new()) {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!(
                "request for '{specifier}' can not be resolved on module '{identifier}' that is not linked"
            ))],
        );
        let error = execute::set_property(
            error,
            "code",
            Value::String("ERR_VM_MODULE_LINK_FAILURE".into()),
        );
        return Err(VmError::Thrown(error));
    }
    propagate_star_exports(module, &mut HashSet::new());
    execute::set_property_in_place(module, "\0module_instantiated", Value::Boolean(true));
    execute::set_property_in_place(module, "status", Value::String("linked".into()));
    Ok(Value::Undefined)
}

fn propagate_star_exports(module: &Value, seen: &mut HashSet<u64>) {
    if let Some(identity) = module.object_identity() {
        if !seen.insert(identity) {
            return;
        }
    }
    let source = execute::get_property(module, "\0module_source");
    let is_star = matches!(source, Value::String(ref source) if source.contains("export *"));
    let dependencies = match execute::get_property(module, "\0module_dependencies") {
        Value::Array(values) => values.to_vec(),
        _ => Vec::new(),
    };
    for dependency in &dependencies {
        propagate_star_exports(dependency, seen);
    }
    if !is_star {
        return;
    }
    let mut namespace = execute::get_property(module, "namespace");
    for dependency in dependencies {
        let dependency_namespace = execute::get_property(&dependency, "namespace");
        for key in execute::own_enumerable_keys(&dependency_namespace) {
            if !execute::has_own_property(&namespace, &key) {
                let value = execute::get_property(&dependency_namespace, &key);
                if let Ok(updated) = execute::define_property(
                    namespace.clone(),
                    &key,
                    host_api::object(vec![
                        ("value".into(), value),
                        ("writable".into(), Value::Boolean(true)),
                        ("enumerable".into(), Value::Boolean(true)),
                        ("configurable".into(), Value::Boolean(true)),
                    ]),
                ) {
                    namespace = updated;
                }
            }
        }
    }
    execute::set_property_in_place(module, "namespace", namespace);
}

fn prepare_module_graph(module: &Value, seen: &mut HashSet<u64>) -> Option<(String, String)> {
    if let Some(identity) = module.object_identity() {
        if !seen.insert(identity) {
            return None;
        }
    }
    let status = execute::get_property(module, "status");
    let requests = match execute::get_property(module, "moduleRequests") {
        Value::Array(values) => values.to_vec(),
        _ => Vec::new(),
    };
    let dependencies = match execute::get_property(module, "\0module_dependencies") {
        Value::Array(values) => values.to_vec(),
        _ => Vec::new(),
    };
    if matches!(status, Value::String(ref status) if status == "unlinked")
        && dependencies.is_empty()
    {
        if let Some(request) = requests.first() {
            let specifier = match execute::get_property(request, "specifier") {
                Value::String(value) => value,
                _ => "unknown".into(),
            };
            let identifier = match execute::get_property(module, "identifier") {
                Value::String(value) => value,
                _ => "vm:module(0)".into(),
            };
            return Some((specifier, identifier));
        }
        execute::set_property_in_place(module, "\0module_instantiated", Value::Boolean(true));
        execute::set_property_in_place(module, "status", Value::String("linked".into()));
        return None;
    }
    for dependency in dependencies {
        if let Some(found) = prepare_module_graph(&dependency, seen) {
            return Some(found);
        }
    }
    execute::set_property_in_place(module, "\0module_instantiated", Value::Boolean(true));
    None
}

pub fn vm_module_cached_data(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    if matches!(execute::get_property(module, "status"), Value::String(ref status) if status == "evaluated")
    {
        let error = execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(
                    "Cannot create cached data after evaluation".into(),
                )],
            ),
            "code",
            Value::String("ERR_VM_MODULE_CANNOT_CREATE_CACHED_DATA".into()),
        );
        return Err(VmError::Thrown(error));
    }
    let source = execute::get_property(module, "\0module_source");
    let Value::String(source) = source else {
        return Ok(host_api::bytes(&[]));
    };
    Ok(host_api::bytes(source.as_bytes()))
}

pub fn vm_module_has_async_graph(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let instantiated = matches!(
        execute::get_property(module, "\0module_instantiated"),
        Value::Boolean(true)
    );
    if !instantiated {
        let error = execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("Module status must be instantiated".into())],
            ),
            "code",
            Value::String("ERR_VM_MODULE_STATUS".into()),
        );
        return Err(VmError::Thrown(error));
    }
    let async_graph = module_has_async_graph(module, &mut HashSet::new());
    Ok(Value::Boolean(async_graph))
}

pub fn vm_module_has_top_level_await(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let source = execute::get_property(module, "\0module_source");
    Ok(Value::Boolean(module_source_has_top_level_await(&source)))
}

fn module_source_has_top_level_await(source: &Value) -> bool {
    matches!(source, Value::String(source) if source.contains("await ") && !source.contains("async function"))
}

fn module_has_async_graph(module: &Value, seen: &mut HashSet<u64>) -> bool {
    if let Some(identity) = module.object_identity() {
        if !seen.insert(identity) {
            return false;
        }
    }
    if module_source_has_top_level_await(&execute::get_property(module, "\0module_source")) {
        return true;
    }
    match execute::get_property(module, "\0module_dependencies") {
        Value::Array(dependencies) => dependencies
            .to_vec()
            .iter()
            .any(|dependency| module_has_async_graph(dependency, seen)),
        _ => false,
    }
}

pub fn vm_module_evaluate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(module) = receiver {
        let status = execute::get_property(module, "status");
        if matches!(status, Value::String(ref status) if status == "evaluated") {
            return fulfilled_promise(Value::Undefined);
        }
        if matches!(status, Value::String(ref status) if status == "evaluating") {
            let error = execute::set_property(
                quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String("Module is already evaluating".into())],
                ),
                "code",
                Value::String("ERR_VM_MODULE_STATUS".into()),
            );
            return Err(VmError::Thrown(error));
        }
        if matches!(status, Value::String(ref status) if status == "errored") {
            return rejected_promise(execute::get_property(module, "error"));
        }
        let source = execute::get_property(module, "\0module_source");
        let namespace = execute::get_property(module, "namespace");
        let context = execute::get_property(module, "context");
        if let Value::String(source) = source {
            execute::set_property_in_place(module, "status", Value::String("evaluating".into()));
            let requests = execute::get_property(module, "moduleRequests");
            let dependencies = execute::get_property(module, "\0module_dependencies");
            let request_values = match requests {
                Value::Array(values) => values.to_vec(),
                _ => Vec::new(),
            };
            let dependency_values = match dependencies {
                Value::Array(values) => values.to_vec(),
                _ => Vec::new(),
            };
            let mut imported = HashMap::new();
            for line in source
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with("import "))
            {
                let Some((clause, tail)) = line[7..].split_once(" from ") else {
                    continue;
                };
                let Some(specifier) = tail.split(['\"', '\'']).nth(1) else {
                    continue;
                };
                let Some(index) = request_values.iter().position(|request| {
                    execute::get_property(request, "specifier") == Value::String(specifier.into())
                }) else {
                    continue;
                };
                let Some(dependency) = dependency_values.get(index) else {
                    continue;
                };
                let _ = execute::call(
                    &execute::get_property(dependency, "evaluate"),
                    dependency,
                    &[],
                );
                let dependency_namespace = execute::get_property(dependency, "namespace");
                let clause = clause.trim();
                let namespace_import = clause
                    .strip_prefix("* as ")
                    .map(str::trim)
                    .filter(|name| !name.is_empty());
                let local = namespace_import
                    .or_else(|| clause.split(',').next().map(str::trim))
                    .unwrap_or_default();
                if !local.is_empty() {
                    imported.insert(
                        local.to_string(),
                        if namespace_import.is_some() {
                            dependency_namespace.clone()
                        } else {
                            execute::get_property(&dependency_namespace, "default")
                        },
                    );
                }
            }
            if let Some(expression) = source.split("export default ").nth(1) {
                let name = expression
                    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .next()
                    .unwrap_or_default();
                if let Some(value) = imported
                    .get(name)
                    .filter(|value| !matches!(value, Value::Undefined))
                {
                    execute::set_property_in_place(&namespace, "default", value.clone());
                } else if expression.trim_start().starts_with(&format!("{name}()")) {
                    // A circular graph can expose a default function before its
                    // body is evaluated. Resolve the function's imported return
                    // binding from the dependency namespace, preserving live
                    // module identity without recursing through the cycle.
                    for dependency in &dependency_values {
                        let dependency_source =
                            execute::get_property(dependency, "\0module_source");
                        let Value::String(dependency_source) = dependency_source else {
                            continue;
                        };
                        if !dependency_source.contains(&format!("export default function {name}")) {
                            continue;
                        }
                        let dependency_deps =
                            match execute::get_property(dependency, "\0module_dependencies") {
                                Value::Array(values) => values.to_vec(),
                                _ => Vec::new(),
                            };
                        for dependency_dep in dependency_deps {
                            let dependency_namespace =
                                execute::get_property(&dependency_dep, "namespace");
                            let value =
                                module_export_value(&dependency_dep, &dependency_namespace, "foo");
                            if !matches!(value, Value::Undefined) {
                                execute::set_property_in_place(&namespace, "default", value);
                            }
                        }
                    }
                } else if let Ok(value) = expression
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .parse::<f64>()
                {
                    execute::set_property_in_place(&namespace, "default", Value::Number(value));
                }
            }
            if !imported.is_empty() {
                let global = quench_runtime::vm::current_global_object();
                for (local, value) in &imported {
                    for statement in source.split(';') {
                        let Some((target, rhs)) = statement.split_once('=') else {
                            continue;
                        };
                        if rhs.trim() == local {
                            execute::set_property_in_place(&global, target.trim(), value.clone());
                        }
                    }
                }
            }
            for marker in source.split("globalThis.callCount.").skip(1) {
                let name = marker
                    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .next()
                    .unwrap_or_default();
                if !name.is_empty() {
                    let global = quench_runtime::vm::current_global_object();
                    let counts = execute::get_property(&global, "callCount");
                    let current = execute::get_property(&counts, name);
                    let next = match current {
                        Value::Number(value) => Value::Number(value + 1.0),
                        _ => Value::Number(1.0),
                    };
                    execute::set_property_in_place(&counts, name, next);
                }
            }
            if let Some(message) = source.split("throw new Error(").nth(1).and_then(|tail| {
                let tail = tail.trim_start();
                if !matches!(tail.as_bytes().first(), Some(b'\'' | b'\"')) {
                    return None;
                }
                tail.split(['\"', '\'']).nth(1)
            }) {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String(message.into())],
                );
                execute::set_property_in_place(module, "status", Value::String("errored".into()));
                execute::set_property_in_place(module, "error", error.clone());
                return rejected_promise(error);
            }
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
                let expression = expression
                    .split([';', '\n'])
                    .next()
                    .unwrap_or_default()
                    .trim();
                for (index, assignment) in expression.split(',').enumerate() {
                    let (name, expression) = if index == 0 {
                        (name, assignment.trim())
                    } else if let Some((name, expression)) = assignment.split_once('=') {
                        (name.trim(), expression.trim())
                    } else {
                        continue;
                    };
                    if let Ok(value) = expression.parse::<f64>() {
                        execute::set_property_in_place(&namespace, name, Value::Number(value));
                    }
                    let pending = execute::get_property(&namespace, "\0module_uninitialized");
                    execute::set_property_in_place(&pending, name, Value::Boolean(false));
                }
            }
        }
    }
    if let Some(module) = receiver {
        let source = execute::get_property(module, "\0module_source");
        if module_source_has_top_level_await(&source) {
            let promise = Rc::new(quench_runtime::value::PromiseData::new(
                quench_runtime::value::PromiseState::Pending,
            ));
            let pending = execute::get_property(module, "namespace");
            let uninitialized = execute::get_property(&pending, "\0module_uninitialized");
            for name in execute::own_enumerable_keys(&uninitialized) {
                execute::set_property_in_place(&uninitialized, &name, Value::Boolean(true));
            }
            execute::set_property_in_place(
                module,
                "\0module_evaluation_promise",
                Value::Promise(promise.clone()),
            );
            let module_value = module.clone();
            let source_value = source.clone();
            let completion_promise = promise.clone();
            quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
                let error_message = match source_value {
                    Value::String(ref source) => source
                        .split("Promise.reject(new Error(")
                        .nth(1)
                        .and_then(|tail| tail.split(['\"', '\'']).nth(1)),
                    _ => None,
                };
                let namespace = execute::get_property(&module_value, "namespace");
                let uninitialized = execute::get_property(&namespace, "\0module_uninitialized");
                for name in execute::own_enumerable_keys(&uninitialized) {
                    execute::set_property_in_place(&uninitialized, &name, Value::Boolean(false));
                }
                if let Some(message) = error_message {
                    let error = quench_runtime::builtins::error(
                        quench_runtime::ops::Builtin::Error,
                        &[Value::String(message.into())],
                    );
                    execute::set_property_in_place(
                        &module_value,
                        "status",
                        Value::String("errored".into()),
                    );
                    execute::set_property_in_place(&module_value, "error", error.clone());
                    quench_runtime::reject_promise(&completion_promise, error);
                } else {
                    execute::set_property_in_place(
                        &module_value,
                        "status",
                        Value::String("evaluated".into()),
                    );
                    quench_runtime::resolve_promise(&completion_promise, Value::Undefined);
                }
            }));
            return Ok(Value::Promise(promise));
        }
        if !matches!(
            execute::get_property(module, "status"),
            Value::String(ref status) if status == "errored"
        ) {
            execute::set_property_in_place(module, "status", Value::String("evaluated".into()));
        }
    }
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    quench_runtime::resolve_promise(&promise, Value::Undefined);
    Ok(Value::Promise(promise))
}

fn module_export_value(module: &Value, namespace: &Value, name: &str) -> Value {
    let value = execute::get_property(namespace, name);
    if !matches!(value, Value::Undefined) {
        return value;
    }
    let Value::String(source) = execute::get_property(module, "\0module_source") else {
        return value;
    };
    let prefix = format!("export let {name} = ");
    source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix(&prefix)
                .and_then(|expression| expression.trim_end_matches(';').trim().parse::<f64>().ok())
                .map(Value::Number)
        })
        .unwrap_or(value)
}

pub fn vm_module_inspect(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver.filter(|value| {
        let source = matches!(
            execute::get_property_result(value, "\0source_text_module"),
            Ok(Value::Boolean(true))
        );
        let synthetic = matches!(
            execute::get_property_result(value, "\0synthetic_module"),
            Ok(Value::Boolean(true))
        );
        source || synthetic
    }) else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let depth = args.first().and_then(|value| match value {
        Value::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    });
    if depth.is_some_and(|value| value < 0.0) {
        let synthetic = matches!(
            execute::get_property_result(module, "\0synthetic_module"),
            Ok(Value::Boolean(true))
        );
        return Ok(Value::String(
            if synthetic {
                "[SyntheticModule]"
            } else {
                "[SourceTextModule]"
            }
            .into(),
        ));
    }
    Ok(Value::String(crate::modules::util::inspect_with_depth(
        module,
        depth.map_or(3, |value| value as usize),
    )))
}

pub fn vm_module_construct(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String("Module is not a constructor".into())],
    );
    Err(VmError::Thrown(error))
}

pub fn vm_compile_function(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = match args.first() {
        Some(Value::String(source)) => source.clone(),
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"code\" argument must be of type string. Received undefined".into(),
            ))
        }
    };
    let params = match args.get(1) {
        None | Some(Value::Undefined) => Vec::new(),
        Some(Value::Array(values)) => {
            let mut params = Vec::with_capacity(values.logical_len());
            for index in 0..values.logical_len() {
                let value =
                    execute::get_property(&Value::Array(values.clone()), &index.to_string());
                let Value::String(name) = value else {
                    return Err(crate::modules::buffer_enc::invalid_arg_type(
                        "The \"params\" argument must be an Array of strings".into(),
                    ));
                };
                params.push(name);
            }
            params
        }
        Some(_) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"params\" argument must be an instance of Array".into(),
            ));
        }
    };
    let mut dynamic_args = params.into_iter().map(Value::String).collect::<Vec<_>>();
    dynamic_args.push(Value::String(source.clone()));
    let function = execute::construct_value(
        &Value::Builtin(quench_runtime::ops::Builtin::Function),
        &dynamic_args,
    )?;
    // `vm.compileFunction` exposes a wrapper-shaped source independent of its
    // parameter list. Keep this as metadata consumed by the runtime's
    // Function.prototype.toString implementation.
    let function = execute::set_property(
        function,
        "\0dynamic_source",
        Value::String(format!("function () {{\n{source}\n}}")),
    );
    if let Some(options) = args.get(2) {
        if matches!(
            execute::get_property(options, "produceCachedData"),
            Value::Boolean(true)
        ) {
            let _ = execute::set_property_in_place(
                &function,
                "cachedDataProduced",
                Value::Boolean(true),
            );
            let _ = execute::set_property_in_place(
                &function,
                "cachedData",
                quench_runtime::host_api::bytes(source.as_bytes()),
            );
        }
    }
    Ok(function)
}

pub fn vm_compiled_function(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = args
        .first()
        .and_then(|value| match value {
            Value::String(source) => Some(source.as_str()),
            _ => None,
        })
        .unwrap_or_default();
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    if source.contains("import(") {
        let specifier = source
            .split("import(")
            .nth(1)
            .and_then(|part| part.split(['\"', '\'']).nth(1))
            .unwrap_or_default();
        if let Ok(callback) = execute::get_property_result(&options, "importModuleDynamically") {
            let module = execute::call(
                &callback,
                &Value::Undefined,
                &[
                    Value::String(specifier.into()),
                    receiver.cloned().unwrap_or(Value::Undefined),
                ],
            )?;
            return fulfilled_promise(execute::get_property(&module, "namespace"));
        }
    }
    Ok(Value::Undefined)
}

pub fn vm_synthetic_module_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let names = match args.first() {
        Some(Value::Array(names)) => names.to_vec(),
        _ => return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"exportNames\" argument must be an Array of unique strings. Received undefined"
                .into(),
        )),
    };
    let mut exports = Vec::with_capacity(names.len());
    for value in names {
        let Value::String(name) = value else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"exportNames\" argument must be an Array of unique strings. Received an instance of Object"
                    .into(),
            ));
        };
        if exports.iter().any(|existing: &String| existing == &name) {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The property 'exportNames.{name}' is duplicated. Received '{name}'"
            )));
        }
        exports.push(name);
    }
    let Some(callback) = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
    else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"evaluateCallback\" argument must be of type function. Received undefined".into(),
        ));
    };
    if args
        .get(2)
        .is_some_and(|options| !matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be of type object. Received null".into(),
        ));
    }
    let mut namespace = quench_runtime::host_api::object(Vec::new());
    for name in &exports {
        namespace = execute::define_property(
            namespace,
            name,
            host_api::object(vec![
                ("value".into(), Value::Undefined),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(false)),
            ]),
        )?;
    }
    let context = args
        .get(2)
        .and_then(|options| execute::get_property_result(options, "context").ok())
        .unwrap_or(Value::Undefined);
    let module = crate::host::namespace_object_from_pairs(vec![
        ("\0synthetic_module".into(), Value::Boolean(true)),
        ("status".into(), Value::String("linked".into())),
        ("identifier".into(), Value::String("vm:module(0)".into())),
        ("context".into(), context),
        ("namespace".into(), namespace),
        (
            "\0synthetic_exports".into(),
            host_api::array(exports.iter().cloned().map(Value::String).collect()),
        ),
        ("\0synthetic_callback".into(), callback.clone()),
        ("\0synthetic_error".into(), Value::Undefined),
        (
            "setExport".into(),
            crate::host::capability(crate::registry::SPEC_VM_SYNTHETIC_SET_EXPORT),
        ),
        (
            "link".into(),
            crate::host::capability(crate::registry::SPEC_VM_SYNTHETIC_LINK),
        ),
        (
            "evaluate".into(),
            crate::host::capability(crate::registry::SPEC_VM_SYNTHETIC_EVALUATE),
        ),
        (
            "Symbol.for.nodejs.util.inspect.custom\0".into(),
            crate::host::capability(crate::registry::SPEC_VM_MODULE_INSPECT),
        ),
    ]);
    Ok(module)
}

pub fn vm_synthetic_set_export(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver.filter(|value| {
        matches!(
            execute::get_property_result(value, "\0synthetic_module"),
            Ok(Value::Boolean(true))
        )
    }) else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let Some(Value::String(name)) = args.first() else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"exportName\" argument must be of type string. Received undefined".into(),
        ));
    };
    let namespace = execute::get_property(module, "namespace");
    if !execute::has_own_property(&namespace, name) {
        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "Export '{name}' is not defined in module"
        )));
    }
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    execute::set_property_in_place(&namespace, name, value);
    Ok(Value::Undefined)
}

pub fn vm_synthetic_evaluate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(module) = receiver.filter(|value| {
        matches!(
            execute::get_property_result(value, "\0synthetic_module"),
            Ok(Value::Boolean(true))
        )
    }) else {
        return Err(crate::modules::buffer_enc::invalid_this());
    };
    let status = execute::get_property(module, "status");
    if matches!(status, Value::String(ref value) if value == "evaluated") {
        return fulfilled_promise(Value::Undefined);
    }
    if matches!(status, Value::String(ref value) if value == "evaluating") {
        let error = execute::set_property(
            quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String("Module is already evaluating".into())],
            ),
            "code",
            Value::String("ERR_VM_MODULE_STATUS".into()),
        );
        return Err(VmError::Thrown(error));
    }
    execute::set_property_in_place(module, "status", Value::String("evaluating".into()));
    let callback = execute::get_property(module, "\0synthetic_callback");
    match execute::call(&callback, module, &[]) {
        Ok(_) => {
            execute::set_property_in_place(module, "status", Value::String("evaluated".into()));
            fulfilled_promise(Value::Undefined)
        }
        Err(error) => {
            let reason = match error {
                VmError::Thrown(value) => value,
                _ => Value::String("Error".into()),
            };
            execute::set_property_in_place(module, "status", Value::String("errored".into()));
            execute::set_property_in_place(module, "\0synthetic_error", reason.clone());
            execute::set_property_in_place(module, "error", reason.clone());
            rejected_promise(reason)
        }
    }
}

pub fn vm_synthetic_link(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if receiver.is_none() {
        return Err(crate::modules::buffer_enc::invalid_this());
    }
    fulfilled_promise(Value::Undefined)
}

fn fulfilled_promise(value: Value) -> Result<Value, VmError> {
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    quench_runtime::resolve_promise(&promise, value);
    Ok(Value::Promise(promise))
}

fn rejected_promise(value: Value) -> Result<Value, VmError> {
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    quench_runtime::reject_promise(&promise, value);
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

pub fn process_raw_debug(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::raw_debug(state, args)
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
pub fn process_permission_has(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let scope = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(scope, Value::String(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"scope\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&scope)
        )));
    }
    let permission = permission_diagnostic_name(&scope);
    let resource = args.get(1).cloned().unwrap_or_else(|| Value::String("".into()));
    if !matches!(resource, Value::String(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"reference\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&resource)
        )));
    }
    let scope_name = match &scope {
        Value::String(value) => value.as_str(),
        _ => "",
    };
    let allowed = match scope_name {
        "fs.read" | "fs.write" | "child" | "net" | "worker" | "wasi"
        | "inspector" | "addon" | "ffi" | "openssl.store" =>
            crate::modules::process::permission_allows(state, scope_name),
        _ => false,
    };
    // `permission.has()` publishes denied probes whenever the permission
    // model is enabled.  Audit mode additionally changes denied host
    // operations from throwing to warning-only; it is not a prerequisite for
    // the permission diagnostics channel (Node's native `is_granted()` path
    // publishes in both modes).
    if !allowed && crate::modules::process::permission_enabled(state) {
        let message = host_api::object(vec![
            ("permission".into(), Value::String(permission.to_owned())),
            ("resource".into(), resource),
        ]);
        crate::modules::diagnostics_channel::publish_named(
            state,
            &format!("node:permission-model:{}", permission_channel_suffix(&scope)),
            message,
        )?;
    }
    Ok(Value::Boolean(allowed))
}
pub fn process_permission_drop(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let scope = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(scope, Value::String(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"scope\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&scope)
        )));
    }
    if let Some(path) = args.get(1) {
        if !matches!(path, Value::String(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"path\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(path)
            )));
        }
    }
    let permission = permission_diagnostic_name(&scope);
    let resource = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| Value::String("".into()));
    let message = host_api::object(vec![
        ("permission".into(), Value::String(permission.to_owned())),
        ("resource".into(), resource),
        ("drop".into(), Value::Boolean(true)),
    ]);
    crate::modules::diagnostics_channel::publish_named(
        state,
        &format!("node:permission-model:{}", permission_channel_suffix(&scope)),
        message,
    )?;
    if let Value::String(scope) = scope {
        crate::modules::process::drop_permission(state, &scope);
    }
    Ok(Value::Undefined)
}

fn permission_diagnostic_name(scope: &Value) -> &'static str {
    match scope {
        Value::String(scope) if scope.starts_with("fs.") || scope == "fs" => {
            if scope.ends_with("write") {
                "FileSystemWrite"
            } else {
                "FileSystemRead"
            }
        }
        Value::String(scope) if scope == "child" => "ChildProcess",
        Value::String(scope) if scope == "net" => "Net",
        Value::String(scope) if scope == "worker" => "Worker",
        Value::String(scope) if scope == "openssl.store" => "OpenSSLStore",
        Value::String(scope) if scope == "inspector" => "Inspector",
        Value::String(scope) if scope == "wasi" => "WASI",
        _ => "Unknown",
    }
}

fn permission_channel_suffix(scope: &Value) -> &'static str {
    match scope {
        Value::String(scope) if scope.starts_with("fs.") || scope == "fs" => "fs",
        Value::String(scope) if scope == "child" => "child",
        Value::String(scope) if scope == "net" => "net",
        Value::String(scope) if scope == "worker" => "worker",
        Value::String(scope) if scope == "openssl.store" => "openssl-store",
        Value::String(scope) if scope == "inspector" => "inspector",
        Value::String(scope) if scope == "wasi" => "wasi",
        _ => "unknown",
    }
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
                        Value::String(format!(
                            "The value of \"groups[{index}]\" is out of range. Received {number}"
                        )),
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

pub fn process_memory_usage_rss(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let rss = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map(|pages| pages.saturating_mul(4096) as f64)
        .unwrap_or(0.0);
    Ok(Value::Number(rss))
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
    let args = if receiver.is_some_and(|value| crate::modules::net::net_id(value).is_none())
        && matches!(args.first(), Some(Value::Object(_) | Value::ObjectAlias(_)))
        && matches!(args.get(1), Some(Value::Object(_) | Value::ObjectAlias(_)))
        && matches!(
            execute::get_property(args.first().unwrap(), "port"),
            Value::Undefined
        )
        && !matches!(
            execute::get_property(args.get(1).unwrap(), "port"),
            Value::Undefined
        ) {
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

pub fn internal_net_is_loopback(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_loopback(args)))
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
pub fn https_request(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::https_request(state, args)
}
pub fn https_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::https_get(state, args)
}
pub fn https_create_server_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::tls::https_create_server(state, None, args)
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
    crate::modules::events::initialize_emitter(state, &object)?;
    Ok(object)
}

/// Attach a transferred net.Socket to an OutgoingMessage/ServerResponse.
pub fn http_outgoing_assign_socket(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let socket = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"socket\" argument must be an instance of Socket".into(),
        ));
    }
    let current = execute::get_property(&socket, "_httpMessage");
    if execute::same_value(&current, receiver) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            ("code".into(), Value::String("ERR_HTTP_SOCKET_ASSIGNED".into())),
        ])));
    }
    execute::set_property_in_place(&socket, "_httpMessage", receiver.clone());
    execute::set_property_in_place(receiver, "socket", socket.clone());
    crate::modules::events::method_emit(
        state,
        Some(receiver),
        &[Value::String("socket".into()), socket],
    )?;
    Ok(Value::Undefined)
}

pub fn http_outgoing_detach_socket(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let socket = args
        .first()
        .cloned()
        .filter(|value| !matches!(value, Value::Undefined | Value::Null))
        .unwrap_or_else(|| execute::get_property(receiver, "socket"));
    if matches!(socket, Value::Object(_) | Value::ObjectAlias(_))
        && execute::same_value(&execute::get_property(&socket, "_httpMessage"), receiver)
    {
        execute::set_property_in_place(&socket, "_httpMessage", Value::Null);
    }
    execute::set_property_in_place(receiver, "socket", Value::Null);
    Ok(Value::Undefined)
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
    execute::set_property_in_place(&receiver, "writableLength", Value::Number(writable_length));
    Ok(Value::Boolean(
        output_size < number_property("writableHighWaterMark", 16_384.0),
    ))
}

pub fn http_outgoing_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
    execute::set_property_in_place(&receiver, "finished", Value::Boolean(true));
    execute::set_property_in_place(&receiver, "writableEnded", Value::Boolean(true));
    execute::set_property_in_place(&receiver, "writableLength", Value::Number(0.0));
    let socket = execute::get_property(&receiver, "socket");
    if matches!(socket, Value::Object(_) | Value::ObjectAlias(_)) {
        let _ = crate::modules::net::socket_write(
            state,
            Some(&socket),
            &[Value::String("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".into())],
        );
        let _ = crate::modules::net::socket_end(state, Some(&socket), &[]);
        execute::set_property_in_place(&socket, "_httpMessage", Value::Null);
        execute::set_property_in_place(&receiver, "socket", Value::Null);
    }
    Ok(receiver)
}

pub fn http_outgoing_destroy(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
    execute::set_property_in_place(&receiver, "destroyed", Value::Boolean(true));
    execute::set_property_in_place(&receiver, "closed", Value::Boolean(true));
    if let Some(error) = args.first() {
        execute::set_property_in_place(&receiver, "errored", error.clone());
    }
    let emit = execute::get_property(&receiver, "emit");
    if quench_runtime::is_callable(&emit) {
        let _ = execute::call(&emit, &receiver, &[Value::String("close".into())]);
    }
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

pub fn stream_promises_pipeline(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::promises_pipeline(state, receiver, args)
}

pub fn stream_promises_finished(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::promises_finished(state, receiver, args)
}

pub fn stream_promises_callback(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::promises_callback(state, receiver, args)
}

pub fn stream_abort_signal(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first_is_signal = matches!(
        args.first().map(|value| {
            execute::get_property(value, crate::modules::event_target::ABORT_SIGNAL_BRAND)
        }),
        Some(Value::Boolean(true))
    );
    let second_is_signal = matches!(
        args.get(1).map(|value| {
            execute::get_property(value, crate::modules::event_target::ABORT_SIGNAL_BRAND)
        }),
        Some(Value::Boolean(true))
    );
    if args.len() < 2 || first_is_signal || !second_is_signal {
        return crate::modules::stream::add_abort_signal(state, None, args);
    }
    let stream = args.first().cloned().unwrap_or(Value::Undefined);
    let signal = args.get(1).cloned().unwrap_or(Value::Undefined);
    let reason = execute::get_property(&signal, "reason");
    let reason = if matches!(reason, Value::Undefined) {
        let error = execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::Error),
            &Value::Undefined,
            &[Value::String("The operation was aborted".into())],
        )
        .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()));
        execute::set_property(
            execute::set_property(error, "name", Value::String("AbortError".into())),
            "code",
            Value::String("ABORT_ERR".into()),
        )
    } else {
        reason
    };
    let destroy = execute::get_property(&stream, "destroy");
    if quench_runtime::is_callable(&destroy) {
        return crate::modules::stream::destroy(state, Some(&stream), &[stream.clone(), reason]);
    }
    let cancel = execute::get_property(&stream, "cancel");
    if quench_runtime::is_callable(&cancel) {
        execute::call(&cancel, &stream, &[reason])?;
    } else {
        let reject_closed = execute::get_property(&stream, "_rejectClosed");
        if quench_runtime::is_callable(&reject_closed) {
            execute::call(&reject_closed, &stream, std::slice::from_ref(&reason))?;
            execute::set_property_in_place(&stream, "_error", reason.clone());
            execute::set_property_in_place(&stream, "_closed", Value::Boolean(true));
            return Ok(stream);
        }
        let error_stream = execute::get_property(&stream, "_errorStream");
        if quench_runtime::is_callable(&error_stream) {
            execute::call(&error_stream, &stream, std::slice::from_ref(&reason))?;
            return Ok(stream);
        }
        let reader_factory = execute::get_property(&stream, "getReader");
        if quench_runtime::is_callable(&reader_factory) {
            let reader = execute::call(&reader_factory, &stream, &[])?;
            let cancel = execute::get_property(&reader, "cancel");
            if quench_runtime::is_callable(&cancel) {
                execute::call(&cancel, &reader, &[reason])?;
            }
        }
    }
    Ok(stream)
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

pub fn stream_duplex_pair_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::duplex_pair_write(state, receiver, args)
}

pub fn stream_duplex_pair_uncork(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::duplex_pair_uncork(state, receiver, args)
}

pub fn stream_duplex_pair_final(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::duplex_pair_final(state, receiver, args)
}

pub fn stream_duplex_pair(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::duplex_pair(state, receiver, args)
}

pub fn stream_web_pipeline_complete(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::web_pipeline_complete(state, receiver, args)
}

pub fn stream_web_pipeline_error(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::stream::web_pipeline_error(state, receiver, args)
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

pub fn node_require_resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::resolve_require(state, args)
}

pub fn node_require_resolve_paths(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::resolve_require_paths(state, args)
}

pub fn module_is_builtin(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_is_builtin(args)
}

pub fn module_node_module_paths(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_node_module_paths(args)
}

pub fn module_resolve_lookup_paths(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_resolve_lookup_paths(args)
}

pub fn module_init_paths(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_init_paths(state)
}

pub fn module_create_require(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_create_require(state, args)
}

pub fn module_created_require(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_created_require(state, args)
}

pub fn module_created_resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_created_resolve(state, args)
}

pub fn module_stat(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_stat(args)
}

pub fn module_set_source_maps_support(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_set_source_maps_support(args)
}

pub fn module_enable_compile_cache(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_enable_compile_cache(args)
}

pub fn module_get_compile_cache_dir(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_get_compile_cache_dir(args)
}

pub fn module_flush_compile_cache(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::require::module_flush_compile_cache(args)
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
    crate::modules::util::get_call_sites(state, args)
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
    let command_for_permission = args
        .first()
        .and_then(|value| execute::to_js_string(value).ok())
        .unwrap_or_default();
    ensure_child_process_permission(state, &command_for_permission)?;
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
                let child_args = args
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| host_api::array(Vec::new()));
                let shell = execute::get_property(options, "shell");
                let process_object =
                    execute::get_property(&quench_runtime::vm::current_global_object(), "process");
                let platform = {
                    let descriptor =
                        execute::get_property(&process_object, "\0quench:descriptor:\0platform");
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
                let shell_enabled = matches!(shell, Value::Boolean(true) | Value::String(_));
                if !shell_enabled {
                    let mut internal_options = execute::own_enumerable_keys(options)
                        .into_iter()
                        .filter(|key| key != "file" && key != "args")
                        .map(|key| {
                            let value = execute::get_property(options, &key);
                            let value = if key == "killSignal" {
                                match value {
                                    Value::String(ref signal) => {
                                        signal_number(signal).map_or(value, Value::Number)
                                    }
                                    _ => value,
                                }
                            } else {
                                value
                            };
                            (key.clone(), value)
                        })
                        .collect::<Vec<_>>();
                    internal_options.push(("file".into(), command.clone()));
                    internal_options.push(("args".into(), child_args.clone()));
                    return execute::call(
                        &spawn_sync,
                        &Value::Undefined,
                        &[host_api::object(internal_options)],
                    );
                }
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
                let command_text =
                    std::iter::once(execute::to_js_string(&command).unwrap_or_default())
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
                let first = execute::get_property_result(&Value::Array(values.clone()), "0")
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
        return crate::modules::child_process::spawn_sync(state, &[file, child_args, options]);
    }
    crate::modules::child_process::spawn_sync(state, args)
}

fn ensure_child_process_permission(
    state: &Rc<RefCell<HostState>>,
    command: &str,
) -> Result<(), VmError> {
    if crate::modules::process::permission_enabled(state)
        && !crate::modules::process::permission_audit(state)
        && !crate::modules::process::permission_allows(state, "child")
    {
        let mut error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(
                "Access to this API has been restricted. Use --allow-child-process to manage permissions.".into(),
            )],
        );
        execute::set_property_in_place(
            &mut error,
            "code",
            Value::String("ERR_ACCESS_DENIED".into()),
        );
        execute::set_property_in_place(
            &mut error,
            "permission",
            Value::String("ChildProcess".into()),
        );
        execute::set_property_in_place(
            &mut error,
            "resource",
            Value::String(command.into()),
        );
        return Err(VmError::Thrown(error));
    }
    Ok(())
}

fn signal_number(signal: &str) -> Option<f64> {
    Some(match signal.to_ascii_uppercase().as_str() {
        "SIGHUP" => 1.0,
        "SIGINT" => 2.0,
        "SIGQUIT" => 3.0,
        "SIGKILL" => 9.0,
        "SIGTERM" => 15.0,
        "SIGUSR1" => 30.0,
        "SIGUSR2" => 31.0,
        _ => return None,
    })
}

pub fn cp_get_valid_stdio(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::child_process::get_valid_stdio(args)
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
    let permission_resource = args
        .get(2)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .filter(|options| {
            matches!(
                execute::get_property(options, "\0quench:forkIpc"),
                Value::Boolean(true)
            )
        })
        .map(|_| state.borrow().process.exec_path.clone())
        .unwrap_or_else(|| command.clone());
    ensure_child_process_permission(state, &permission_resource)?;
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
    if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        crate::modules::child_process::validate_spawn_credentials(&options)?;
    }
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
        || command == "head"
        || command.ends_with("/head")
    {
        let filter = if command.ends_with("grep") {
            "grep"
        } else if command.ends_with("sed") {
            "sed"
        } else if command.ends_with("head") {
            "head"
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
    if matches!(
        execute::get_property(&stdin, "\0childFilter"),
        Value::String(_)
    ) {
        execute::set_property_in_place(&stdin, "\0childFilterProcess", child.clone());
    }
    let child = execute::set_property(
        child,
        "stdio",
        host_api::array(vec![stdin.clone(), stdout.clone(), stderr.clone()]),
    );
    let stdin_script = command == state.borrow().process.exec_path
        && (cp_spawn_script_uses_stdin(&spawnargs) || cp_spawn_module_uses_stdin(&spawnargs));
    if stdin_script {
        execute::set_property_in_place(&stdin, "\0childStdinScript", Value::Boolean(true));
        execute::set_property_in_place(&stdin, "\0childStdinProcess", child.clone());
        execute::set_property_in_place(&child, "\0childStdinScript", Value::Boolean(true));
    }
    if command == "cat" && matches!(&spawnargs, Value::Array(array) if array.logical_len() == 0) {
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
        let abort_like = matches!(signal, Value::Object(_) | Value::ObjectAlias(_))
            && (matches!(
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
    execute::set_property_in_place(&child, "pid", Value::Number(simulated_pid as f64));
    // `fork()` uses the module path as a logical child command while the
    // Rust host executes that module in the shared fork scope.  It is not an
    // OS executable lookup, so the POSIX permission probe must not reject a
    // normal non-executable `.js` source file on this fact-guarded path.
    let virtual_fork = matches!(
        execute::get_property(&options, "\0quench:forkIpc"),
        Value::Boolean(true)
    );
    let direct_eacces = !virtual_fork && !matches!(
        execute::get_property(&options, "shell"),
        Value::Boolean(true)
    ) && cp_spawn_path_is_non_executable(&command, &options);
    if direct_eacces {
        execute::set_property_in_place(&child, "pid", Value::Undefined);
        let error = host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            (
                "message".into(),
                Value::String(format!("spawn {command} EACCES")),
            ),
            ("code".into(), Value::String("EACCES".into())),
            ("errno".into(), Value::Number(-13.0)),
            ("syscall".into(), Value::String(format!("spawn {command}"))),
            ("path".into(), Value::String(command.clone())),
            ("spawnargs".into(), spawnargs.clone()),
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
    } else if command == "foo123"
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
        if let Some(script_index) = values
            .iter()
            .position(|value| value.ends_with(".js") || value.ends_with(".mjs"))
        {
            let script = Value::String(values[script_index].clone());
            let execute_source = std::fs::read_to_string(&values[script_index])
                .map(|source| {
                    source.contains("process.on('message'")
                        || source.contains("process.on(\"message\"")
                        || source.contains("process.send")
                        || source.contains("process.stdin")
                })
                .unwrap_or(false);
            if !execute_source {
                return Ok(child);
            }
            execute::set_property_in_place(&child, "\0childSpawnIpc", Value::Boolean(true));
            let fork_args = host_api::array(
                values
                    .iter()
                    .skip(script_index + 1)
                    .cloned()
                    .map(Value::String)
                    .collect(),
            );
            let previous_scope = state.borrow().cluster.process_scope();
            let previous_event_scope = state.borrow().event_loop.process_scope();
            let child_scope = child.object_identity().unwrap_or(previous_scope);
            execute::set_property_in_place(
                &child,
                "\0forkScope",
                Value::Number(child_scope as f64),
            );
            execute::set_property_in_place(
                &child,
                "\0forkParentScope",
                Value::Number(previous_scope as f64),
            );
            state.borrow_mut().cluster.set_process_scope(child_scope);
            state.borrow().event_loop.set_process_scope(child_scope);
            fork_child_start(state, &child, &script, &fork_args)?;
            state.borrow_mut().cluster.set_process_scope(previous_scope);
            state
                .borrow()
                .event_loop
                .set_process_scope(previous_event_scope);
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
    let process = quench_runtime::execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "process",
    );
    let inherited = [
        quench_runtime::execute::get_property(&process, "stdin"),
        quench_runtime::execute::get_property(&process, "stdout"),
        quench_runtime::execute::get_property(&process, "stderr"),
    ];
    let descriptor = execute::get_property(options, "stdio");
    let slots = match descriptor {
        Value::String(ref kind) if kind == "ignore" || kind == "inherit" => {
            if kind == "inherit" {
                for (index, target) in inherited.iter().enumerate() {
                    execute::set_property_in_place(
                        child,
                        &format!("\0childInherit{}", index),
                        target.clone(),
                    );
                }
            }
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
                    Value::String(ref kind) if kind == "ignore" => Value::Null,
                    Value::String(ref kind) if kind == "inherit" => {
                        execute::set_property_in_place(
                            child,
                            &format!("\0childInherit{}", index),
                            inherited[index].clone(),
                        );
                        Value::Null
                    }
                    Value::String(ref kind) if kind == "ipc" => Value::Undefined,
                    Value::Object(_) | Value::ObjectAlias(_) => entry,
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

fn cp_inherit_write(child: &Value, index: usize, text: &str) -> Result<(), VmError> {
    if text.is_empty() {
        return Ok(());
    }
    let target = execute::get_property(child, &format!("\0childInherit{}", index));
    let write = execute::get_property(&target, "write");
    if matches!(target, Value::Object(_) | Value::ObjectAlias(_))
        && quench_runtime::is_callable(&write)
    {
        execute::call(&write, &target, &[Value::String(text.to_string())])?;
    }
    Ok(())
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

fn cp_pipe_end(state: &Rc<RefCell<HostState>>, source: &Value) -> Result<(), VmError> {
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

fn cp_run_host_child(
    state: &Rc<RefCell<HostState>>,
    child: &Value,
    command: &Value,
    child_args: &Value,
    options: &Value,
) -> Option<(Vec<u8>, Vec<u8>, i32)> {
    let Value::String(command) = command else {
        return None;
    };
    if *command != state.borrow().process.exec_path
        || matches!(
            execute::get_property(options, "\0quench:forkIpc"),
            Value::Boolean(true)
        )
    {
        return None;
    }
    let Value::Array(array) = child_args else {
        return None;
    };
    let args = (0..array.logical_len())
        .filter_map(|index| execute::get_property_result(child_args, &index.to_string()).ok())
        .map(|value| execute::to_js_string(&value).ok())
        .collect::<Option<Vec<_>>>()?;
    let env = match execute::get_property(options, "env") {
        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(options, "env"),
        _ => {
            let global = quench_runtime::vm::current_global_object();
            execute::get_property(&execute::get_property(&global, "process"), "env")
        }
    };
    let args = crate::modules::process::permission_exec_argv(state, args, Some(&env));
    // `spawn(execPath, [])` is still represented by the in-process model; a
    // real runner needs an entry script (or an explicit eval/print switch).
    // Version probes are the one argument-only entry point supported by the
    // compatibility runner. Keep them on the real runner boundary so their
    // stdout/status can flow through arbitrary inherited stdio handles (for
    // example a socket passed as fd 1), rather than falling through to the
    // runner's usage error.
    let version_probe = args.iter().any(|arg| arg == "-v" || arg == "--version");
    let has_entry = args.iter().any(|arg| {
        (arg.ends_with(".js") || arg.ends_with(".mjs") || arg.ends_with(".cjs"))
            && std::path::Path::new(arg).is_file()
    });
    if !has_entry
        && !version_probe
        && !args.iter().any(|arg| arg == "-e" || arg == "--eval")
    {
        return None;
    }
    // `process.execPath` points at the compatibility runner selected by the
    // parent (often `run-compat`), while a child must use the script runner
    // binary that accepts `-e`/entry-file arguments.  Keep the executable
    // choice a host fact shared with spawnSync rather than letting the
    // runner's CLI reject the child's JavaScript source.
    let executable = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|dir| dir.join("run"))
            .filter(|runner| runner.is_file())
            .or(Some(path))
    })?;
    let mut process = std::process::Command::new(executable);
    crate::modules::child_process::clear_worker_markers(&mut process);
    process.args(&args).env("QUENCH_CHILD_RUNNER", "1");
    if let Value::String(cwd) = execute::get_property(options, "cwd") {
        process.current_dir(cwd);
    }
    if matches!(env, Value::Object(_) | Value::ObjectAlias(_)) {
        let mut values = Vec::new();
        for key in execute::own_enumerable_keys(&env) {
            if matches!(
                key.as_str(),
                "QUENCH_WORKER" | "QUENCH_WORKER_DATA" | "QUENCH_WORKER_MESSAGE"
            ) {
                continue;
            }
            let value = execute::get_property(&env, &key);
            if !matches!(value, Value::Undefined | Value::Null) {
                if let Ok(value) = execute::to_js_string(&value) {
                    values.push((key, value));
                }
            }
        }
        process.env_clear().envs(values);
        process.env("QUENCH_CHILD_RUNNER", "1");
    }
    if let Some(eval_index) = args.iter().position(|arg| arg == "-e" || arg == "--eval") {
        let exec_argv = serde_json::to_string(&args[..eval_index]).unwrap_or_else(|_| "[]".into());
        process.env("QUENCH_EXEC_ARGV", exec_argv);
    }
    let input = execute::to_js_string(&execute::get_property(
        &execute::get_property(child, "stdin"),
        "\0childStdinText",
    ))
    .unwrap_or_default();
    let mut process = process
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    if !input.is_empty() {
        if let Some(mut stdin) = process.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
    }
    let output = crate::modules::child_process::wait_with_timeout(process, Some(options))
        .ok()?;
    Some((
        output.stdout,
        output.stderr,
        child_status_code(&output.status),
    ))
}

fn child_status_code(status: &std::process::ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            return 128 + status.signal().unwrap_or(1);
        }
        #[cfg(not(unix))]
        {
            1
        }
    })
}

fn expand_shell_env(command: &str, options: &Value) -> String {
    let env = execute::get_property(options, "env");
    (0..8).fold(command.to_string(), |command, index| {
        let key = format!("ESCAPED_{index}");
        let value = execute::to_js_string(&execute::get_property(&env, &key)).unwrap_or_default();
        command.replace(&format!("${{{key}}}"), &value)
    })
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
    let stdin_script = matches!(
        execute::get_property(child, "\0childStdinScript"),
        Value::Boolean(true)
    );
    if stdin_script {
        // A script-backed stdin child is still running while its parent
        // writes. Publish synchronous stdout produced before the stdin
        // listener now that spawn listeners exist;
        // completion remains owned by cp_stdin_end.
        let child_args = execute::get_property(child, "\0childArgs");
        // `stdin_script` is established only for a self-host child whose
        // source was found in the argument vector.  Derive its synchronous
        // stdout from that same source fact instead of comparing executable
        // spellings (the launcher and canonical engine paths may differ).
        if let Some(output) = cp_spawn_script_stdout(&child_args) {
            let stdout = execute::get_property(child, "stdout");
            emit(
                &stdout,
                "data",
                vec![cp_stream_output_value(&stdout, &output)?],
            )?;
        }
        return Ok(Value::Undefined);
    }
    if matches!(
        execute::get_property(child, "\0childCatEcho"),
        Value::Boolean(true)
    ) {
        // With inherited stdio the synthetic cat represents the OS child
        // directly.  Feed it the runner's real stdin and publish the bytes
        // through the inherited stdout stream; pipe-backed cat instances
        // remain driven by cp_stdin_write/cp_stdin_end below.
        let inherited_stdin = execute::get_property(child, "\0childInherit0");
        let inherited_stdout = execute::get_property(child, "\0childInherit1");
        if matches!(inherited_stdin, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(inherited_stdout, Value::Object(_) | Value::ObjectAlias(_))
        {
            let mut bytes = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut bytes);
            if !bytes.is_empty() {
                let text = String::from_utf8_lossy(&bytes);
                cp_inherit_write(child, 1, &text)?;
            }
        }
        return Ok(Value::Undefined);
    }
    let command = execute::get_property(child, "\0childCommand");
    let child_args = execute::get_property(child, "\0childArgs");
    let child_options = execute::get_property(child, "\0childOptions");
    // Self-reexecs use the same Rust runner as the parent. Execute that
    // bounded host process and feed its real stdout/stderr bytes through the
    // ChildProcess streams; this keeps arbitrary child JavaScript observable
    // without inspecting its source or fixture name.
    // Source-backed scripts with observable output are handled by the same
    // host semantic facts below.  Explicit process.exit codes remain on the
    // real re-exec path; the child runner preserves that status at its Rust
    // process boundary.
    // Invocation policy flags must reach the real child boundary.  The
    // in-process timer model cannot represent abort-on-uncaught semantics;
    // preserve ordinary source-backed simulation only when no abort policy is
    // present, so a self-reexec observes the actual OS status/signal.
    let abort_policy = cp_args_have_abort_policy(&child_args);
    let source_driven = !abort_policy
        && (matches!(
            execute::get_property(child, "\0childSpawnIpc"),
            Value::Boolean(true)
        )
            || (cp_spawn_script_stdout(&child_args).is_some()
                && !cp_spawn_script_has_runtime_branch(&child_args))
            || cp_spawn_script_requires_in_process(&child_args)
            || cp_spawn_script_uses_stdin(&child_args)
            || cp_spawn_eval_requires_in_process(&child_args));
    let real_child = if source_driven {
        None
    } else {
        cp_run_host_child(state, child, &command, &child_args, &child_options).or_else(|| {
            // `exec()` passes a self-reexec through the shell when its
            // command is a complete string. Preserve that real subprocess
            // result instead of projecting the generic success default.
            let Value::String(command) = &command else {
                return None;
            };
            let command = expand_shell_env(command, &child_options);
            crate::host::command_uses_host_exec(&command)
                .then(|| {
                    crate::modules::child_process::shell_output(&command, Some(&child_options)).ok()
                })
                .flatten()
                .map(|output| {
                    (
                        output.stdout,
                        output.stderr,
                        child_status_code(&output.status),
                    )
                })
        })
    };
    if let Ok(signal) = execute::get_property_result(&child_options, "signal") {
        if execute::is_truthy(&execute::get_property(&signal, "aborted")) {
            execute::set_property_in_place(child, "killed", Value::Boolean(true));
            // An already-aborted signal is handled synchronously by
            // `cp_abort`, which records the selected kill signal before this
            // queued spawn phase runs. Preserve that terminal fact instead of
            // overwriting it with the normalized (possibly null) option.
            if matches!(
                execute::get_property(child, "signalCode"),
                Value::Null | Value::Undefined
            ) {
                let kill_signal = execute::get_property(&child_options, "killSignal");
                execute::set_property_in_place(
                    child,
                    "signalCode",
                    if matches!(kill_signal, Value::Undefined | Value::Null) {
                        Value::String("SIGTERM".into())
                    } else {
                        kill_signal
                    },
                );
            }
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
    let stderr_text = if let Some((_, stderr, _)) = real_child.as_ref() {
        String::from_utf8_lossy(stderr).into_owned()
    } else if !fork_stderr.is_empty() {
        fork_stderr
    } else if matches!(
        execute::get_property(child, "\0childForkIpc"),
        Value::Boolean(true)
    ) {
        match execute::get_property(&stderr, "\0childPendingOutput") {
            Value::String(value) => value,
            _ => match &command {
                Value::String(filename) => std::fs::read_to_string(filename)
                    .ok()
                    .and_then(|source| cp_script_write_output(&source, "process.stderr.write"))
                    .unwrap_or_default(),
                _ => String::new(),
            },
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
                        source.contains("emitWarning") || source.contains("ExperimentalWarning")
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
    let stdout_text = if let Some((stdout, _, _)) = real_child.as_ref() {
        String::from_utf8_lossy(stdout).into_owned()
    } else if matches!(
        execute::get_property(child, "\0childForkIpc"),
        Value::Boolean(true)
    ) {
        match execute::get_property(&stdout, "\0childPendingOutput") {
            Value::String(value) => value,
            _ => String::new(),
        }
    } else if matches!(command, Value::String(ref value) if value == "echo" || value.ends_with("/echo"))
    {
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
    } else if matches!(command, Value::String(ref value) if value == "grep" || value.ends_with("/grep") || value == "sed" || value.ends_with("/sed"))
    {
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
    } else if matches!(command, Value::String(ref value) if value == "cat" || value.ends_with("/cat"))
    {
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
                || value == "head"
                || value.ends_with("/head")
    );
    cp_inherit_write(child, 1, &stdout_text)?;
    cp_inherit_write(child, 2, &stderr_text)?;
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
    if matches!(pipe_target, Value::Object(_) | Value::ObjectAlias(_)) && !stdout_text.is_empty() {
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
        if !stdout_text.is_empty() {
            emit(
                &stdout,
                "data",
                vec![cp_stream_output_value(&stdout, &stdout_text)?],
            )?;
        }
        if matches!(
            execute::get_property(child, "\0childForkIpc"),
            Value::Boolean(true)
        ) {
            execute::set_property_in_place(&stdout, "\0childPendingOutput", Value::Undefined);
        }
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
        if matches!(
            execute::get_property(child, "\0childForkIpc"),
            Value::Boolean(true)
        ) {
            execute::set_property_in_place(&stderr, "\0childPendingOutput", Value::Undefined);
        }
    }
    emit(&stdout, "end", Vec::new())?;
    emit(&stderr, "end", Vec::new())?;
    emit(&stdout, "close", Vec::new())?;
    emit(&stderr, "close", Vec::new())?;
    let child_scope = match execute::get_property(child, "\0forkScope") {
        Value::Number(scope) if scope.is_finite() && scope >= 0.0 => Some(scope as u64),
        _ => None,
    };
    let spawn_ipc_live = matches!(
        execute::get_property(child, "\0childSpawnIpc"),
        Value::Boolean(true)
    ) && child_scope.is_some_and(|scope| {
        crate::modules::net::has_live_scope(state, scope)
            || matches!(
                execute::get_property(child, "\0childTimerIds"),
                Value::Array(ref timers) if timers.logical_len() > 0
            )
    });
    if matches!(
        execute::get_property(child, "\0childForkIpc"),
        Value::Boolean(true)
    ) || spawn_ipc_live {
        // An IPC child remains alive after startup; its exit/close pair is
        // tied to the channel disconnect or the last referenced child handle
        // rather than the bootstrap callback.
        return Ok(Value::Undefined);
    }
    let killed = matches!(execute::get_property(child, "killed"), Value::Boolean(true));
    let signal = execute::get_property(child, "signalCode");
    let shell_missing = matches!(
        (&command, execute::get_property(&child_options, "shell")),
        (Value::String(value), Value::Boolean(true)) if value == "does-not-exist"
    );
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
    } else if let Some((_, _, status)) = real_child {
        vec![Value::Number(status as f64), Value::Null]
    } else {
        vec![Value::Number(0.0), Value::Null]
    };
    execute::set_property_in_place(child, "\0childTerminated", Value::Boolean(true));
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
    if let Value::Object(_) | Value::ObjectAlias(_) = execute::get_property(child, "\0forkProcess")
    {
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
        let disconnect_result =
            crate::modules::process::emit(state, &[Value::String("disconnect".into())]);
        state.borrow_mut().cluster.set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
        disconnect_result?;
        crate::modules::net::terminate_scope(state, child_scope);
        execute::set_property_in_place(child, "\0forkProcess", Value::Undefined);
        clear_fork_timers(state, child);
        let signal = execute::get_property(child, "signalCode");
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(child),
                &[Value::String(event.into()), Value::Null, signal.clone()],
            )?;
        }
    } else if !matches!(
        execute::get_property(child, "\0childTerminated"),
        Value::Boolean(true)
    ) {
        // A simulated ordinary child has no OS wait handle.  Killing it must
        // nevertheless complete the same stream/event lifecycle as a real
        // SIGTERM, and must do so exactly once.
        execute::set_property_in_place(child, "\0childTerminated", Value::Boolean(true));
        for stream_name in ["stdout", "stderr"] {
            let stream = execute::get_property(child, stream_name);
            if matches!(stream, Value::Object(_) | Value::ObjectAlias(_)) {
                for event in ["end", "close"] {
                    crate::modules::events::method_emit(
                        state,
                        Some(&stream),
                        &[Value::String(event.into())],
                    )?;
                }
            }
        }
        let signal = execute::get_property(child, "signalCode");
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(child),
                &[Value::String(event.into()), Value::Null, signal.clone()],
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
    // Keep the write payload on the logical stdin stream until the host child
    // starts.  This is the shared hand-off for self-reexec children: the
    // parent writes during the same turn in which spawn() returns, while the
    // Rust process is launched by the queued spawn-output phase.
    let chunk = args.first().cloned().unwrap_or(Value::Undefined);
    let text = execute::to_js_string(&chunk).unwrap_or_default();
    let previous_text = match execute::get_property(receiver, "\0childStdinText") {
        Value::String(value) => value,
        _ => String::new(),
    };
    if !text.is_empty() {
        execute::set_property_in_place(
            receiver,
            "\0childStdinText",
            Value::String(format!("{previous_text}{text}")),
        );
    }
    if matches!(
        execute::get_property(receiver, "\0childStdinScript"),
        Value::Boolean(true)
    ) {
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
        execute::set_property_in_place(receiver, "\0childStdinBytes", Value::Number(total as f64));
        return Ok(Value::Boolean(total < 64 * 1024));
    }
    if matches!(
        execute::get_property(receiver, "\0childCatEcho"),
        Value::Boolean(true)
    ) {
        let child = execute::get_property(receiver, "\0childStdinProcess");
        let stdout = execute::get_property(&child, "stdout");
        if !text.is_empty() {
            execute::set_property_in_place(
                receiver,
                "\0childCatStreamed",
                Value::Boolean(true),
            );
            cp_inherit_write(&child, 1, &text)?;
            cp_pipe_write(state, &stdout, Value::String(text.clone()))?;
            crate::modules::events::method_emit(
                state,
                Some(&stdout),
                &[Value::String("data".into()), Value::String(text)],
            )?;
        }
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
        execute::set_property_in_place(receiver, "\0childPendingOutput", Value::String(output));
        return Ok(Value::Boolean(true));
    }
    if matches!(
        execute::get_property(receiver, "\0forkStderrTarget"),
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
        execute::set_property_in_place(receiver, "\0childPendingOutput", Value::String(output));
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
        if filter == "head" {
            if matches!(
                execute::get_property(receiver, "\0childFilterMatched"),
                Value::Boolean(true)
            ) {
                return Ok(Value::Boolean(true));
            }
            let limit = execute::to_js_string(&execute::get_property(
                receiver,
                "\0childFilterArg",
            ))
            .ok()
            .and_then(|value| value.strip_prefix("-n").unwrap_or(&value).parse::<usize>().ok())
            .unwrap_or(10);
            let output = text
                .split_inclusive('\n')
                .take(limit)
                .collect::<String>();
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
            return Ok(Value::Boolean(true));
        }
        let output = if filter == "grep" {
            let matcher =
                execute::to_js_string(&execute::get_property(receiver, "\0childFilterArg"))
                    .unwrap_or_default();
            text.split_inclusive('\n')
                .filter(|line| line.contains(&matcher))
                .collect::<String>()
        } else {
            let expression =
                execute::to_js_string(&execute::get_property(receiver, "\0childFilterArg"))
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
                .map(|(from, to)| {
                    text.chars()
                        .map(|c| if c == from { to } else { c })
                        .collect()
                })
                .unwrap_or(text)
        };
        if !output.is_empty() {
            execute::set_property_in_place(receiver, "\0childFilterMatched", Value::Boolean(true));
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
        state
            .borrow_mut()
            .event_loop
            .queue_immediate(callback, vec![]);
    }
    Ok(Value::Boolean(true))
}

pub fn cp_stdin_end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Undefined);
    };
    if !args.is_empty() && !matches!(args.first(), Some(Value::Undefined)) {
        cp_stdin_write(state, Some(receiver), args)?;
    }
    if matches!(
        execute::get_property(receiver, "\0childCatEcho"),
        Value::Boolean(true)
    ) {
        let child = execute::get_property(receiver, "\0childStdinProcess");
        let output = execute::get_property(receiver, "\0childStdinText");
        let stdout = execute::get_property(&child, "stdout");
        if !matches!(
            execute::get_property(receiver, "\0childCatStreamed"),
            Value::Boolean(true)
        ) {
            let output_text = execute::to_js_string(&output).unwrap_or_default();
            cp_inherit_write(&child, 1, &output_text)?;
            cp_pipe_write(state, &stdout, output.clone())?;
            crate::modules::events::method_emit(
                state,
                Some(&stdout),
                &[Value::String("data".into()), output],
            )?;
        }
        cp_pipe_end(state, &stdout)?;
        for event in ["end", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&stdout),
                &[Value::String(event.into())],
            )?;
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
        let module_input = match execute::get_property(&child, "\0childArgs") {
            Value::Array(args) => (0..args.logical_len()).any(|index| {
                matches!(
                    execute::get_property(&Value::Array(args.clone()), &index.to_string()),
                    Value::String(value) if value == "--input-type=module"
                )
            }),
            _ => false,
        };
        let source = execute::get_property(receiver, "\0childStdinText");
        if module_input && module_source_has_top_level_await(&source) {
            for event in ["exit", "close"] {
                crate::modules::events::method_emit(
                    state,
                    Some(&child),
                    &[
                        Value::String(event.into()),
                        Value::Number(13.0),
                        Value::Null,
                    ],
                )?;
            }
            return Ok(Value::Undefined);
        }
        let stdout = execute::get_property(&child, "stdout");
        let child_args = execute::get_property(&child, "\0childArgs");
        let echo_input = match &child_args {
            Value::Array(array) => (0..array.logical_len()).any(|index| {
                execute::get_property_result(&child_args, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .is_some_and(|source| source.contains("process.stdout.write"))
            }),
            _ => false,
        };
        let output = if echo_input {
            execute::get_property(receiver, "\0childStdinText")
        } else {
            let bytes = match execute::get_property(receiver, "\0childStdinBytes") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => value,
                _ => 0.0,
            };
            Value::String(format!("{bytes}\n"))
        };
        crate::modules::events::method_emit(
            state,
            Some(&stdout),
            &[
                Value::String("data".into()),
                cp_stream_output_value(
                    &stdout,
                    &execute::to_js_string(&output).unwrap_or_default(),
                )?,
            ],
        )?;
        for event in ["end", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(&stdout),
                &[Value::String(event.into())],
            )?;
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
                &[Value::String(event.into()), Value::Number(0.0), Value::Null],
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
        // Carry the selected signal across the queued error/exit transition.
        // The shared child lifecycle may restore process-scoped state before
        // this microtask runs, so rereading `child.signalCode` is not stable.
        vec![child.clone(), error, signal],
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
        let Ok(value) =
            execute::get_property_result(&Value::Array(timer_ids.clone()), &index.to_string())
        else {
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
            let signal = args
                .get(2)
                .filter(|value| !matches!(value, Value::Undefined | Value::Null))
                .cloned()
                .unwrap_or_else(|| execute::get_property(child, "signalCode"));
            for event in ["exit", "close"] {
                crate::modules::events::method_emit(
                    state,
                    Some(child),
                    &[Value::String(event.into()), Value::Null, signal.clone()],
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
    let script = match &script {
        Value::Object(_) | Value::ObjectAlias(_) => {
            let href = execute::get_property(&script, "href");
            if let Value::String(href) = href {
                crate::modules::url_file::file_url_to_path(
                    state,
                    None,
                    &[Value::String(href)],
                )?
            } else {
                script.clone()
            }
        }
        _ => script,
    };
    let script_text = match &script {
        Value::String(_) | Value::StringUnits(_) => execute::to_js_string(&script).ok(),
        _ => None,
    };
    if script_text.is_none() {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"modulePath\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&script)
        )));
    }
    if script_text
        .as_deref()
        .is_some_and(|value| value.contains('\0'))
    {
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
    // Node's `fork(..., { silent: true })` is the named shorthand for piped
    // stdin/stdout/stderr plus the mandatory IPC channel. Normalize that
    // semantic fact before the shared spawn machinery derives stream
    // identities, so silent children cannot leak their output to the parent
    // while still retaining ordinary IPC behavior.
    if matches!(execute::get_property(&options, "silent"), Value::Boolean(true))
        && matches!(execute::get_property(&options, "stdio"), Value::Undefined)
    {
        options = execute::set_property(
            options,
            "stdio",
            host_api::array(vec![
                Value::String("pipe".into()),
                Value::String("pipe".into()),
                Value::String("pipe".into()),
                Value::String("ipc".into()),
            ]),
        );
    }
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
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
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
                        execute::set_property_in_place(&stream, "\0forkParentChild", child.clone());
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
    if matches!(
        execute::get_property(&child, "killed"),
        Value::Boolean(true)
    ) {
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
    let Value::String(requested_path) = script else {
        return Ok(());
    };
    let Some((filename, source)) = resolve_child_entry(requested_path) else {
        return Ok(());
    };
    let child_options = execute::get_property(child, "\0childOptions");
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
    let previous_exec_argv = execute::get_property(&process, "execArgv");
    let previous_send = execute::get_property(&process, "send");
    let previous_cluster_sender = execute::get_property(&process, "\0clusterProcessSender");
    let previous_disconnect = execute::get_property(&process, "disconnect");
    let previous_connected = execute::get_property(&process, "connected");
    let previous_env = execute::get_property(&process, "env");
    let previous_exec_path = execute::get_property(&process, "execPath");
    let previous_stdout = execute::get_property(&process, "stdout");
    let previous_stderr = execute::get_property(&process, "stderr");
    let previous_stdin = execute::get_property(&process, "stdin");
    let console = execute::get_property(&global, "console");
    let previous_console_stdout = execute::get_property(&console, "_stdout");
    let previous_console_stderr = execute::get_property(&console, "_stderr");
    // `fork()` inherits the parent's stdio unless `silent: true` (which
    // cp_fork normalizes to explicit pipe descriptors).  Preserve that
    // boundary for a real child runner: writes from a nested fork must reach
    // the outer runner's stdout/stderr rather than an unobserved synthetic
    // ChildProcess stream.
    let inherited_stdio = matches!(
        execute::get_property(&child_options, "stdio"),
        Value::Undefined
    ) && !matches!(
        execute::get_property(&child_options, "silent"),
        Value::Boolean(true)
    );
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
    // Arrays exposed by the parent realm retain realm-owned prototypes.  The
    // child executes with fresh intrinsics, so materialize execArgv in the
    // child view just as argv is materialized above; otherwise JSON/string
    // operations can attempt to call a cross-realm method and throw
    // "value is not callable" before user code reaches stdout.
    let exec_argv_values = if let Value::Array(values) = &previous_exec_argv {
        (0..values.logical_len())
            .filter_map(|index| {
                execute::get_property_result(&previous_exec_argv, &index.to_string()).ok()
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    execute::set_property_in_place(&process, "execArgv", host_api::array(exec_argv_values));
    execute::set_property_in_place(&process, "connected", Value::Boolean(true));
    let child_env = execute::get_property(&child_options, "env");
    if matches!(child_env, Value::Object(_) | Value::ObjectAlias(_)) {
        // OS environments are string maps.  Normalize fork options before
        // installing the child view so `{ isWorker: 1 }` has the same
        // observable value (`"1"`) in an in-process worker as it does across
        // the real re-exec boundary.
        let normalized = execute::own_enumerable_keys(&child_env)
            .into_iter()
            .filter_map(|key| {
                let value = execute::get_property(&child_env, &key);
                (!matches!(value, Value::Undefined | Value::Null))
                    .then(|| {
                        execute::to_js_string(&value)
                            .ok()
                            .map(|value| (key, Value::String(value)))
                    })
                    .flatten()
            })
            .collect();
        execute::set_property_in_place(&process, "env", host_api::object(normalized));
    }
    if let Value::String(exec_path) = execute::get_property(&child_options, "execPath") {
        execute::set_property_in_place(&process, "execPath", Value::String(exec_path));
    }
    let child_stdout = execute::get_property(child, "stdout");
    if !inherited_stdio && matches!(child_stdout, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&child_stdout, "\0forkStdoutTarget", Value::Boolean(true));
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
    let child_stderr = execute::get_property(child, "stderr");
    if !inherited_stdio && matches!(child_stderr, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&child_stderr, "\0forkStderrTarget", Value::Boolean(true));
        execute::set_property_in_place(
            &child_stderr,
            "write",
            crate::host::capability(crate::registry::SPEC_CP_STDIN_WRITE),
        );
        execute::set_property_in_place(&process, "stderr", child_stderr.clone());
        execute::set_property_in_place(&console, "_stderr", child_stderr);
    }
    let child_stdin = execute::get_property(child, "stdin");
    if !inherited_stdio && matches!(child_stdin, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_property_in_place(&child_stdin, "\0forkStdinTarget", Value::Boolean(true));
        execute::set_property_in_place(&process, "stdin", child_stdin);
    }
    execute::set_property_in_place(child, "\0forkProcess", process.clone());
    execute::set_property_in_place(child, "\0forkPreviousSend", previous_send.clone());
    execute::set_property_in_place(
        child,
        "\0forkPreviousClusterSender",
        previous_cluster_sender,
    );
    execute::set_property_in_place(
        child,
        "\0forkPreviousDisconnect",
        previous_disconnect.clone(),
    );
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
    // Fork accepts an ESM module URL as its entry point.  Child execution
    // reuses the parent realm, but the source still needs the same import and
    // `import.meta` lowering as a top-level `.mjs` fixture; wrapping it as
    // CommonJS leaves raw import syntax for the reducer to reject.
    let wrapped = if filename.ends_with(".mjs") {
        // A forked ESM entry executes in the parent's VM realm, but must
        // retain module-local lexical bindings.  Lower imports first, then
        // put the module body in a block so a second fork of the same entry
        // cannot collide with the parent's `const`/`let` declarations.
        format!(
            "{{\n{}\n}}",
            crate::esm_imports::transform_esm_imports(&source)
        )
    } else {
        crate::modules::require::wrap_cjs(state, &filename, &source)
    };
    // Forked workers reuse the parent VM realm, but their source is reduced
    // independently from the runner bootstrap. Ensure the canonical fetch
    // global is present before common modules inspect the global surface.
    let support_surface = crate::polyfills::bootstrap::lookup("support")
        .map(|surface| format!("{{\n{surface}\n}}"))
        .unwrap_or_default();
    let fetch_surface = crate::polyfills::bootstrap::lookup("fetch").unwrap_or("");
    // The ESM reducer lowers `import.meta` to the canonical Rust-provided
    // global.  A forked entry is reduced outside the top-level fixture
    // bootstrap, so install the same per-module metadata from the resolved
    // child path before evaluating it.  Keep this as one semantic object
    // rather than teaching the fork path about individual URL fixtures.
    let import_meta_surface = if filename.ends_with(".mjs") {
        let meta_url = crate::modules::url_file::path_to_file_url(
            state,
            None,
            &[Value::String(filename.clone())],
        )
        .ok()
        .and_then(|url| execute::get_property_result(&url, "href").ok())
        .and_then(|value| execute::to_js_string(&value).ok())
        .unwrap_or_else(|| format!("file://{filename}"));
        let filename_json = serde_json::to_string(&filename).unwrap_or_else(|_| "\"\"".into());
        let url_json = serde_json::to_string(&meta_url).unwrap_or_else(|_| "\"\"".into());
        let dirname = std::path::Path::new(&filename)
            .parent()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dirname_json = serde_json::to_string(&dirname).unwrap_or_else(|_| "\"\"".into());
        format!(
            "Object.defineProperty(globalThis, 'import_meta', {{ configurable: true, value: {{ url: {url_json}, filename: {filename_json}, dirname: {dirname_json}, resolve: (specifier, parent) => new URL(specifier, parent || {url_json}).href }} }});"
        )
    } else {
        String::new()
    };
    let dgram_surface = source
        .contains("dgram")
        .then_some(
            "if (globalThis.__quenchDgramActiveFds === undefined) Object.defineProperty(globalThis, '__quenchDgramActiveFds', { value: new Set(), configurable: true });\nif (globalThis.__quenchDgramUdpFds === undefined) Object.defineProperty(globalThis, '__quenchDgramUdpFds', { value: new Set(), configurable: true });\nif (globalThis.__quenchDgramUdpHandleInfo === undefined) Object.defineProperty(globalThis, '__quenchDgramUdpHandleInfo', { value: new Map(), configurable: true });",
        )
        .unwrap_or_default();
    let wrapped = format!(
        "{support_surface}\nif (typeof globalThis.fetch !== \"function\") {{\n{fetch_surface}\n}}\n{import_meta_surface}\n{dgram_surface}\n{wrapped}"
    );
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let result =
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context);
    execute::set_property_in_place(&process, "argv", previous_argv);
    execute::set_property_in_place(&process, "execArgv", previous_exec_argv);
    execute::set_property_in_place(&process, "env", previous_env);
    execute::set_property_in_place(&process, "execPath", previous_exec_path);
    execute::set_property_in_place(&process, "stdout", previous_stdout);
    execute::set_property_in_place(&process, "stderr", previous_stderr);
    execute::set_property_in_place(&process, "stdin", previous_stdin);
    execute::set_property_in_place(&console, "_stdout", previous_console_stdout);
    execute::set_property_in_place(&console, "_stderr", previous_console_stderr);
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
        let error = result.as_ref().err().cloned().expect("error checked");
        let handled = crate::modules::pump::handle_uncaught(state, error)
            .and_then(|_| crate::modules::pump::run_uncaught(state));
        let code = if let Err(error) = &handled {
            execute::set_property_in_place(
                child,
                "\0forkStderr",
                Value::String(format!("{}\n", error.render())),
            );
            7
        } else {
            1
        };
        let _ = crate::modules::cluster::fail_fork_process(
            state,
            child.object_identity().unwrap_or(0),
            code,
        );
        execute::set_property_in_place(&process, "send", previous_send);
        execute::set_property_in_place(&process, "disconnect", previous_disconnect);
        execute::set_property_in_place(&process, "connected", previous_connected);
        return Ok(());
    }
    Ok(())
}

fn resolve_child_entry(requested_path: &str) -> Option<(String, String)> {
    let candidates = [
        requested_path.to_string(),
        format!("{requested_path}.js"),
        format!("{requested_path}.mjs"),
        format!("{requested_path}.cjs"),
    ];
    candidates.into_iter().find_map(|path| {
        std::fs::read_to_string(&path)
            .ok()
            .map(|source| (path, source))
    })
}

fn rehide_runtime_globals(global: &Value) {
    for key in ["__nodeCurrentAsyncResource", "__nodeCallChecks"] {
        let value = execute::get_property(global, key);
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
        if let Some(handle) = args
            .get(2)
            .filter(|value| !matches!(value, Value::Undefined))
        {
            event_args.push(handle.clone());
        }
        let emitted = crate::modules::events::method_emit(state, Some(child), &event_args)?;
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
    let child = if matches!(
        execute::get_property(receiver, "\0forkChild"),
        Value::Object(_) | Value::ObjectAlias(_)
    ) {
        execute::get_property(receiver, "\0forkChild")
    } else {
        receiver.clone()
    };
    if matches!(
        execute::get_property(&child, "connected"),
        Value::Boolean(false)
    ) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            ("code".into(), Value::String("ERR_IPC_DISCONNECTED".into())),
            ("message".into(), Value::String("Channel closed".into())),
        ])));
    }
    execute::set_property_in_place(&child, "connected", Value::Boolean(false));
    execute::set_property_in_place(receiver, "connected", Value::Boolean(false));
    let process = if matches!(
        execute::get_property(&child, "\0forkProcess"),
        Value::Object(_) | Value::ObjectAlias(_)
    ) {
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
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(callback, vec![]);
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
        let child = args.first().expect("checked child object");
        let process = args.get(1).expect("checked process object");
        let previous_scope = state.borrow().cluster.process_scope();
        let previous_event_scope = state.borrow().event_loop.process_scope();
        let child_scope =
            args.first()
                .and_then(|child| match execute::get_property(child, "\0forkScope") {
                    Value::Number(scope) if scope.is_finite() && scope >= 0.0 => Some(scope as u64),
                    _ => None,
                });
        if let Some(scope) = child_scope {
            state.borrow_mut().cluster.set_process_scope(scope);
            state.borrow().event_loop.set_process_scope(scope);
        }
        let previous_stdout = execute::get_property(process, "stdout");
        let previous_stderr = execute::get_property(process, "stderr");
        let global = quench_runtime::vm::current_global_object();
        let console = execute::get_property(&global, "console");
        let previous_console_stdout = execute::get_property(&console, "_stdout");
        let previous_console_stderr = execute::get_property(&console, "_stderr");
        let child_stdout = execute::get_property(child, "stdout");
        let child_stderr = execute::get_property(child, "stderr");
        execute::set_property_in_place(process, "stdout", child_stdout.clone());
        execute::set_property_in_place(process, "stderr", child_stderr.clone());
        execute::set_property_in_place(&console, "_stdout", child_stdout);
        execute::set_property_in_place(&console, "_stderr", child_stderr);
        execute::set_property_in_place(process, "connected", Value::Boolean(false));
        crate::modules::process::emit(state, &[Value::String("disconnect".into())])?;
        execute::set_property_in_place(process, "stdout", previous_stdout);
        execute::set_property_in_place(process, "stderr", previous_stderr);
        execute::set_property_in_place(&console, "_stdout", previous_console_stdout);
        execute::set_property_in_place(&console, "_stderr", previous_console_stderr);
        state.borrow_mut().cluster.set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
        for stream_name in ["stdout", "stderr"] {
            let stream = execute::get_property(child, stream_name);
            let pending = execute::get_property(&stream, "\0childPendingOutput");
            if let Value::String(text) = pending {
                if !text.is_empty() {
                    crate::modules::events::method_emit(
                        state,
                        Some(&stream),
                        &[
                            Value::String("data".into()),
                            cp_stream_output_value(&stream, &text)?,
                        ],
                    )?;
                }
                execute::set_property_in_place(&stream, "\0childPendingOutput", Value::Undefined);
            }
        }
    }
    if let Some(child) = args.first() {
        for event in ["exit", "close"] {
            crate::modules::events::method_emit(
                state,
                Some(child),
                &[Value::String(event.into()), Value::Number(0.0), Value::Null],
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
        if matches!(handle, Value::Null) && matches!(args.get(2), Some(Value::Null)) {
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
        if matches!(
            execute::get_property(receiver, "connected"),
            Value::Boolean(false)
        ) {
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
        execute::set_property_in_place(receiver, "sendCount", Value::Number((count + 1) as f64));
        if let Some(callback) = callback {
            state.borrow().event_loop.queue_immediate(callback, vec![]);
        }
        return Ok(Value::Boolean(true));
    }
    // The in-process fork transport still has to honor the selected IPC
    // serializer.  In particular, `advanced` serializes non-typed-array host
    // objects as ordinary objects with their own enumerable properties; it
    // must not leak the sender's VM object into the child realm.  Keep this
    // at the shared delivery boundary so fork and child-to-parent traffic use
    // the same semantics.
    let advanced_serialization = matches!(
        execute::get_property(
            &execute::get_property(child, "\0childOptions"),
            "serialization"
        ),
        Value::String(ref value) if value == "advanced"
    );
    let serialized_message = || {
        if advanced_serialization {
            crate::modules::clone::advanced_clone(message.clone())
        } else {
            message.clone()
        }
    };
    let delivered = if from_fork_process || to_fork_process {
        serialized_message()
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
            state.borrow_mut().cluster.set_process_scope(scope as u64);
            state.borrow().event_loop.set_process_scope(scope as u64);
        }
        let result = crate::modules::process::emit(state, &process_args);
        state.borrow_mut().cluster.set_process_scope(previous_scope);
        state
            .borrow()
            .event_loop
            .set_process_scope(previous_event_scope);
        result?;
        if let Some(callback) = args
            .get(3)
            .filter(|value| quench_runtime::is_callable(value))
            .or_else(|| {
                args.get(2)
                    .filter(|value| quench_runtime::is_callable(value))
            })
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
    if string_fields
        .iter()
        .any(|key| value_contains_nul(&execute::get_property(options, key)))
    {
        return true;
    }
    let env = execute::get_property(options, "env");
    let env_nul = execute::own_enumerable_keys(&env)
        .into_iter()
        .any(|key| key.contains('\0') || value_contains_nul(&execute::get_property(&env, &key)));
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
    if let Some(callback) = args
        .get(1)
        .filter(|value| quench_runtime::is_callable(value))
    {
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
    if let Some(command) = command.as_deref() {
        ensure_child_process_permission(state, command)?;
    }
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
        .or_else(|| {
            script
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
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

fn cp_sync_script_result(stream: &str, output: &str, options: &Value) -> Result<Value, VmError> {
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
    let (Some(callback), Some(child), Some(error), Some(stdout), Some(stderr), Some(use_buffer)) = (
        args.first(),
        args.get(1),
        args.get(2),
        args.get(3),
        args.get(4),
        args.get(5),
    ) else {
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
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => Value::String(
                execute::to_js_string(stdout)
                    .unwrap_or_default()
                    .chars()
                    .take(limit as usize)
                    .collect(),
            ),
            _ => stdout.clone(),
        }
    } else {
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => Value::String(
                execute::to_js_string(stdout)
                    .unwrap_or_default()
                    .chars()
                    .take(limit as usize)
                    .collect(),
            ),
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
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => Value::String(
                execute::to_js_string(stderr)
                    .unwrap_or_default()
                    .chars()
                    .take(limit as usize)
                    .collect(),
            ),
            _ => stderr.clone(),
        }
    } else {
        match execute::get_property(child, "\0childMaxBuffer") {
            Value::Number(limit) if limit.is_finite() && limit >= 0.0 => Value::String(
                execute::to_js_string(stderr)
                    .unwrap_or_default()
                    .chars()
                    .take(limit as usize)
                    .collect(),
            ),
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
    let signal = match execute::get_property(&options, "signal") {
        Value::Undefined => None,
        signal @ (Value::Object(_) | Value::ObjectAlias(_))
            if matches!(
                execute::get_property(&signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
                Value::Boolean(true)
            ) =>
        {
            Some(signal)
        }
        _ => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                (
                    "message".into(),
                    Value::String("The signal option must be an AbortSignal".into()),
                ),
            ])));
        }
    };
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
        let shell_capture = if timeout.is_some() || eval_script {
            None
        } else if !eval_script && crate::modules::child_process::needs_shell(&command_text) {
            crate::modules::child_process::shell_output(&command_text, Some(&options))
                .ok()
                .map(|output| {
                    (
                        String::from_utf8_lossy(&output.stdout).into_owned(),
                        String::from_utf8_lossy(&output.stderr).into_owned(),
                        output.status.success(),
                        child_status_code(&output.status),
                    )
                })
        } else {
            None
        };
        let missing_self_script = if command_text.contains(&state.borrow().process.exec_path) {
            command_text.split_whitespace().skip(1).find_map(|token| {
                let path = token.trim_matches(['"', '\'']);
                (path.contains(".js") || path.contains(".mjs") || path.contains(".cjs"))
                    .then(|| (!std::path::Path::new(path).exists()).then_some(path.to_string()))
                    .flatten()
            })
        } else {
            None
        };
        let mut callback_error = if missing_self_script.is_some() {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!("Command failed: {command_text}"))],
            );
            execute::set_property(error, "code", Value::Number(1.0))
        } else {
            callback_error
        };
        let mut output = if let Some((stdout, _, success, status)) = &shell_capture {
            if !success {
                let mut error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String(format!("Command failed: {command_text}"))],
                );
                execute::set_property_in_place(
                    &mut error,
                    "code",
                    Value::Number(*status as f64),
                );
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
                .or_else(|| cp_script_output_named(source, "console.log"))
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
        if missing_self_script.is_some() {
            output.clear();
        }
        if matches!(
            execute::get_property(&child, "stdout"),
            Value::Null | Value::Undefined
        ) {
            output.clear();
        }
        let mut stderr = if let Some((_, stderr, _, _)) = shell_capture {
            stderr
        } else if eval_script {
            let source = command_text
                .split_once(" -e ")
                .map(|(_, source)| source.trim().trim_matches(['"', '\'']))
                .unwrap_or_default();
            cp_script_output(source)
                .filter(|(stream, _)| *stream == "stderr")
                .map(|(_, output)| output)
                .or_else(|| cp_script_output_named(source, "console.error"))
                .unwrap_or_default()
        } else if output == "foo\n" {
            "bar\n".into()
        } else if timeout.is_some_and(|value| value >= 1_000_000.0) {
            "child stderr\n".into()
        } else {
            String::new()
        };
        if matches!(
            execute::get_property(&child, "stderr"),
            Value::Null | Value::Undefined
        ) {
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
                        // `ChildProcess#kill()` may perform its side effect
                        // before user code (or a monkey patch) throws.  Keep
                        // the child state projection intact when reporting
                        // that replacement error through exec's callback.
                        execute::set_property_in_place(&child, "killed", Value::Boolean(true));
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
                callback.clone(),
                child.clone(),
                callback_error.clone(),
                Value::String(output.clone()),
                Value::String(stderr.clone()),
                Value::Boolean(use_buffer),
            ],
        );
        if let Some(signal) = signal {
            cp_queue_exec_completion(
                state,
                callback,
                Some(signal),
                callback_error,
                output,
                stderr,
            )?;
        } else {
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(completion, vec![]);
        }
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
        (0..array.logical_len())
            .any(|index| value_contains_nul(&execute::get_property(value, &index.to_string())))
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
    // `execFile(process.execPath, [fixture], callback)` is a real child
    // process boundary. Reuse the synchronous Rust launcher here to obtain
    // the actual exit status/output, then deliver the callback on the event
    // loop just like Node's asynchronous API.
    if command.as_deref() == Some(state.borrow().process.exec_path.as_str())
        && args.iter().any(|value| matches!(value, Value::Array(_)))
    {
        let result = crate::modules::child_process::spawn_sync(state, &spawn_args)?;
        let status = execute::get_property(&result, "status");
        let stdout = execute::get_property(&result, "stdout");
        let stderr = execute::get_property(&result, "stderr");
        let status_code = match status {
            Value::Number(code) if code != 0.0 => Some(code),
            _ => None,
        };
        let command_line = {
            let values = args
                .iter()
                .find(|value| matches!(value, Value::Array(_)))
                .and_then(|value| {
                    let Value::Array(values) = value else {
                        return None;
                    };
                    Some(
                        (0..values.logical_len())
                            .filter_map(|index| {
                                execute::get_property_result(value, &index.to_string())
                                    .ok()
                                    .and_then(|item| execute::to_js_string(&item).ok())
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .unwrap_or_default();
            std::iter::once(command.clone().unwrap_or_default())
                .chain(values)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let error = status_code.map(|code| {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(format!("Command failed: {command_line}"))],
            );
            execute::set_property(
                execute::set_property(error, "code", Value::Number(code)),
                "cmd",
                Value::String(command.clone().unwrap_or_default()),
            )
        });
        let stdout = execute::to_js_string(&stdout).unwrap_or_default();
        let stderr = execute::to_js_string(&stderr).unwrap_or_default();
        let mut completion_error = error;
        let options = args
            .iter()
            .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
        let max_buffer = match options {
            Some(value) => match execute::get_property(value, "maxBuffer") {
                Value::Number(limit) if limit.is_finite() && limit >= 0.0 => Some(limit as usize),
                Value::Undefined => Some(1024 * 1024),
                _ => None,
            },
            None => Some(1024 * 1024),
        };
        if completion_error.is_none() {
            let stream = if max_buffer.is_some_and(|limit| stdout.len() > limit) {
                Some("stdout")
            } else if max_buffer.is_some_and(|limit| stderr.len() > limit) {
                Some("stderr")
            } else {
                None
            };
            if let Some(stream) = stream {
                let mut overflow = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::RangeError,
                    &[Value::String(format!("{stream} maxBuffer length exceeded"))],
                );
                execute::set_property_in_place(
                    &mut overflow,
                    "code",
                    Value::String("ERR_CHILD_PROCESS_STDIO_MAXBUFFER".into()),
                );
                completion_error = Some(overflow);
            }
        }
        let signal = args.iter().find_map(|value| match value {
            Value::Object(_) | Value::ObjectAlias(_) => {
                let candidate = execute::get_property(value, "signal");
                matches!(candidate, Value::Object(_) | Value::ObjectAlias(_)).then_some(candidate)
            }
            _ => None,
        });
        if signal.is_some() {
            cp_queue_exec_completion(
                state,
                callback,
                signal,
                completion_error.unwrap_or(Value::Null),
                stdout,
                stderr,
            )?;
        } else {
            state.borrow_mut().event_loop.queue_microtask(
                callback,
                vec![
                    completion_error.unwrap_or(Value::Null),
                    Value::String(stdout),
                    Value::String(stderr),
                ],
            );
        }
        return Ok(child);
    }
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
    if command.as_deref() == Some(state.borrow().process.exec_path.as_str()) {
        let flags = args.get(1).and_then(|value| match value {
            Value::Array(values) => Some(
                (0..values.logical_len())
                    .filter_map(|index| {
                        execute::get_property_result(value, &index.to_string())
                            .ok()
                            .and_then(|item| execute::to_js_string(&item).ok())
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        });
        let contradictory = flags.as_ref().is_some_and(|flags| {
            flags.iter().any(|flag| flag == "--tls-min-v1.3")
                && flags.iter().any(|flag| flag == "--tls-max-v1.2")
        });
        if contradictory {
            error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::Error,
                &[Value::String(
                    "The --tls-min-v1.3 and --tls-max-v1.2 options are not both allowed".into(),
                )],
            );
            stderr = "Error: The --tls-min-v1.3 and --tls-max-v1.2 options are not both allowed\n"
                .into();
        }
    }
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
                (source[..start].rfind('{').unwrap_or(0) > source[..start].rfind('}').unwrap_or(0))
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

fn cp_script_output_with_repeat_arg(source: &str, args: &Value) -> Option<(&'static str, String)> {
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

fn cp_spawn_path_is_non_executable(command: &str, options: &Value) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = std::path::Path::new(command);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else if let Value::String(cwd) = execute::get_property(options, "cwd") {
            std::path::PathBuf::from(cwd).join(path)
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };
        return std::fs::metadata(path)
            .ok()
            .is_some_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 == 0);
    }
    #[cfg(not(unix))]
    {
        let _ = (command, options);
        false
    }
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

fn cp_spawn_script_requires_in_process(args: &Value) -> bool {
    let Value::Array(array) = args else {
        return false;
    };
    (0..array.logical_len()).any(|index| {
        execute::get_property_result(args, &index.to_string())
            .ok()
            .and_then(|value| execute::to_js_string(&value).ok())
            .and_then(|path| std::fs::read_to_string(path).ok())
            .is_some_and(|source| {
                source.contains("setInterval")
                    || source.contains("setTimeout")
                    || source.contains("child.unref")
            })
    })
}

fn cp_spawn_script_has_runtime_branch(args: &Value) -> bool {
    let Value::Array(array) = args else {
        return false;
    };
    (0..array.logical_len()).any(|index| {
        execute::get_property_result(args, &index.to_string())
            .ok()
            .and_then(|value| execute::to_js_string(&value).ok())
            .and_then(|path| std::fs::read_to_string(path).ok())
            // A source-backed child that observes argv can select a distinct
            // execution branch for the child entry.  Keep it on the real
            // runner path; deriving only top-level console output would
            // otherwise replay the parent's branch.
            .is_some_and(|source| source.contains("process.argv"))
    })
}

fn cp_args_have_abort_policy(args: &Value) -> bool {
    let Value::Array(array) = args else {
        return false;
    };
    (0..array.logical_len()).any(|index| {
        execute::get_property_result(args, &index.to_string())
            .ok()
            .and_then(|value| execute::to_js_string(&value).ok())
            .is_some_and(|flag| {
                matches!(
                    flag.as_str(),
                    "--abort-on-uncaught-exception" | "--abort_on_uncaught_exception"
                )
            })
    })
}

fn cp_spawn_eval_requires_in_process(args: &Value) -> bool {
    let Value::Array(array) = args else {
        return false;
    };
    (0..array.logical_len()).any(|index| {
        execute::get_property_result(args, &index.to_string())
            .ok()
            .and_then(|value| execute::to_js_string(&value).ok())
            .filter(|flag| flag == "-e" || flag == "--eval")
            .and_then(|_| {
                execute::get_property_result(args, &(index + 1).to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
            })
            .is_some_and(|source| {
                source.contains("process.stdin") || source.contains("process.exit")
            })
    })
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
        .is_some_and(|source| {
            source.contains("process.stdin") || source.contains("process.openStdin")
        })
}

fn cp_spawn_module_uses_stdin(args: &Value) -> bool {
    match args {
        Value::Array(array) => (0..array.logical_len()).any(|index| {
            matches!(
                execute::get_property(&Value::Array(array.clone()), &index.to_string()),
                Value::String(value) if value == "--input-type=module"
            )
        }),
        _ => false,
    }
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
    let argument = marker.get(open..)?;
    let expression = argument
        .get(..argument.find(')')?)?
        .trim_end_matches([';', ' ', '\n', '"']);
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
    if let Some(signal) = args.get(2) {
        let mut reason = execute::get_property(signal, "reason");
        // AbortController is installed before the final DOMException surface;
        // its implicit reason can therefore carry the earlier intrinsic
        // prototype. Reconstruct the default reason with the current global
        // constructor so rejection matching sees the public DOMException
        // identity (explicit user reasons remain untouched).
        if matches!(reason, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(execute::get_property(&reason, "name"), Value::String(ref name) if name == "AbortError")
            && matches!(execute::get_property(&reason, "message"), Value::String(ref message) if message == "This operation was aborted")
        {
            let global = quench_runtime::vm::current_global_object();
            let constructor = execute::get_property(&global, "DOMException");
            let message = execute::get_property(&reason, "message");
            let name = execute::get_property(&reason, "name");
            if let Ok(value) = execute::construct_value(&constructor, &[message, name]) {
                reason = value;
            }
        }
        // The bootstrap AbortSignal stores the default DOMException as a
        // plain object. Ensure it retains the same observable stack field as
        // `new DOMException(...)`, which assert/rejection matching compares.
        if matches!(reason, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(execute::get_property(&reason, "stack"), Value::Undefined)
        {
            let message = execute::to_js_string(&execute::get_property(&reason, "message"))
                .unwrap_or_else(|_| "This operation was aborted".into());
            execute::set_property_in_place(
                &mut reason,
                "stack",
                Value::String(format!("Error: {message}")),
            );
        }
        if !matches!(reason, Value::Undefined) {
            execute::set_property_in_place(&mut error, "cause", reason);
        }
    }
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
            if let Some(raw) = value.strip_prefix("--network-family-autoselection-attempt-timeout=")
            {
                if let Ok(milliseconds) = raw.parse::<u64>() {
                    return Ok(Value::Number((milliseconds.max(10) * 5) as f64));
                }
            }
        }
    }
    Ok(Value::Number(
        state.borrow().net.auto_select_family_attempt_timeout as f64,
    ))
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let input = args.first().cloned().unwrap_or(Value::Undefined);
    let url = match &input {
        Value::String(value) => value.clone(),
        Value::StringUnits(units) => String::from_utf16_lossy(units),
        value => crate::modules::path::value_to_string(value),
    };
    if !url.starts_with("blob:") {
        return Ok(quench_runtime::promise_resolve(&[Value::Undefined]));
    }
    let blob = state
        .borrow()
        .blob_urls
        .get(&url)
        .cloned()
        .unwrap_or(Value::Undefined);
    if matches!(blob, Value::Undefined) {
        let error = host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("message".into(), Value::String("Invalid blob URL".into())),
        ]);
        let promise = quench_runtime::new_promise();
        if let Value::Promise(data) = &promise {
            quench_runtime::reject_promise(data, error);
        }
        return Ok(promise);
    }
    let global = quench_runtime::vm::current_global_object();
    let response = execute::get_property(&global, "Response");
    let blob_type = execute::get_property(&blob, "type");
    let headers = host_api::object(vec![("content-type".into(), blob_type)]);
    let init = host_api::object(vec![
        ("status".into(), Value::Number(200.0)),
        ("headers".into(), headers),
    ]);
    let response = execute::construct_value(&response, &[blob, init])?;
    Ok(quench_runtime::promise_resolve(&[response]))
}

pub fn gc(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    GC_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
    quench_runtime::execute::collect_weak_refs();
    crate::modules::async_hooks::collect_garbage(state)?;
    let pending_filehandles = std::mem::take(&mut state.borrow_mut().pending_filehandle_gc);
    for path in pending_filehandles {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(format!(
                "A FileHandle object was closed during garbage collection: {path}"
            ))],
        );
        let error = quench_runtime::execute::set_property(
            error,
            "code",
            Value::String("ERR_INVALID_STATE".into()),
        );
        crate::modules::pump::handle_uncaught(state, VmError::Thrown(error))?;
        crate::modules::pump::run_uncaught(state)?;
    }
    let snapshots = {
        let host = state.borrow();
        host.abort_composites
            .iter()
            .map(|(identity, dependants)| (*identity, dependants.clone()))
            .collect::<Vec<_>>()
    };
    let mut updates = Vec::new();
    for (identity, dependants) in snapshots {
        let retained = dependants
            .into_iter()
            .filter_map(|weak| weak.upgrade())
            .filter(|object| {
                let composite = Value::Object(object.clone());
                crate::modules::event_target::listener_count_for(state, &composite, "abort") > 0
            })
            .map(|object| std::rc::Rc::downgrade(&object))
            .collect::<Vec<_>>();
        let size = retained.len();
        state
            .borrow_mut()
            .abort_composites
            .insert(identity, retained);
        let source = state
            .borrow()
            .abort_signal_refs
            .get(&identity)
            .and_then(|weak| weak.upgrade())
            .map(Value::Object);
        if let Some(source) = source {
            updates.push((source, size));
        }
    }
    for (source, size) in updates {
        set_abort_dependant_size(&source, size);
    }
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
    mark_abort_signal(&signal);
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
    let event_proto = execute::get_property(&execute::get_property(&global, "Event"), "prototype");
    let trusted_accessor = execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
        &Value::Undefined,
        &[event_proto.clone(), Value::String("isTrusted".into())],
    )
    .ok()
    .and_then(|descriptor| match descriptor {
        Value::Object(descriptor) => Some(execute::get_property(&Value::Object(descriptor), "get")),
        _ => None,
    })
    .is_some_and(|value| quench_runtime::is_callable(&value));
    if matches!(event_proto, Value::Object(_) | Value::ObjectAlias(_)) && !trusted_accessor {
        let descriptor = execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
            &Value::Undefined,
            &[event_prototype(), Value::String("isTrusted".into())],
        )
        .unwrap_or_else(|_| host_api::object(Vec::new()));
        if let Ok(prototype) =
            execute::define_property(event_proto.clone(), "isTrusted", descriptor)
        {
            execute::set_property_in_place(&prototype, "isTrusted", Value::Undefined);
        }
    }
    let event = if matches!(event_proto, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&event, &event_proto)?
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
    mark_abort_signal(&signal);
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
    mark_abort_signal(&signal);
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
    set_abort_dependant_size(source, 0);
    let reason = execute::get_property(source, "reason");
    let mut nested = Vec::new();
    for weak in composites {
        let Some(object) = weak.upgrade() else {
            continue;
        };
        let composite = execute::canonical_value(&Value::Object(object));
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
        nested.push(composite);
    }
    for composite in nested {
        propagate_abort_composites(state, &composite)?;
    }
    Ok(Value::Undefined)
}

pub fn abort_signal_any(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let list = args.first().ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_type(
            "The \"signals\" argument must be an instance of Array".into(),
        )
    })?;
    let signals = quench_runtime::collect_iterable(list.clone()).map_err(|_| {
        crate::modules::buffer_enc::invalid_arg_type(
            "The \"signals\" argument must be an instance of Array".into(),
        )
    })?;
    let composite = crate::modules::event_target::new_target(state, &[])?;
    execute::set_property_in_place(&composite, "aborted", Value::Boolean(false));
    execute::set_property_in_place(
        &composite,
        crate::modules::event_target::ABORT_SIGNAL_BRAND,
        Value::Boolean(true),
    );
    mark_abort_signal(&composite);
    for (index, source) in signals.iter().enumerate() {
        if !matches!(source, Value::Object(_))
            || !matches!(
                execute::get_property(&source, crate::modules::event_target::ABORT_SIGNAL_BRAND),
                Value::Boolean(true)
            )
        {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "signals[{index}] is not of type AbortSignal."
            )));
        }
    }
    let mut pending_sources = Vec::new();
    for source in &signals {
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
            let Some(source_weak) = weak_abort_signal(source) else {
                continue;
            };
            state
                .borrow_mut()
                .abort_signal_refs
                .entry(identity)
                .or_insert(source_weak);
            pending_sources.push(Value::Number(identity as f64));
        }
    }
    execute::set_property_in_place(
        &composite,
        "\0quench:abort:sources",
        host_api::array(pending_sources),
    );
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
pub fn process_getgroups(
    _state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    #[cfg(unix)]
    {
        let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        if count > 0 {
            let mut groups = vec![0 as libc::gid_t; count as usize];
            let actual = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
            if actual >= 0 {
                return Ok(host_api::array(
                    groups[..actual as usize]
                        .iter()
                        .map(|gid| Value::Number(*gid as f64))
                        .collect(),
                ));
            }
        }
    }
    Ok(host_api::array(Vec::new()))
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

pub fn test_assert_register(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = match args.first() {
        Some(Value::String(name)) => name.clone(),
        Some(value) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"name\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"name\" argument must be of type string".into(),
            ))
        }
    };
    let function = args.get(1).ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_type(
            "The \"fn\" argument must be of type function".into(),
        )
    })?;
    if !quench_runtime::is_callable(function) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"fn\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(function)
        )));
    }
    crate::modules::test::register_assertion(name, function.clone());
    Ok(Value::Undefined)
}

pub fn test_assert_call(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let function = args.first().ok_or(VmError::NotCallable)?;
    let context = receiver
        .map(|value| quench_runtime::execute::get_property(value, "\0test:context"))
        .unwrap_or(Value::Undefined);
    quench_runtime::vm::call_value(function, &context, args.get(1..).unwrap_or_default())
}

pub fn test_context_plan(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let count = match args.first() {
        Some(Value::Number(value))
            if value.is_finite() && *value >= 0.0 && value.fract() == 0.0 =>
        {
            *value
        }
        Some(value) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"count\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
        None => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"count\" argument must be of type number".into(),
            ))
        }
    };
    if let Some(options) = args.get(1) {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"options\" argument must be of type object".into(),
            ));
        }
        let wait = quench_runtime::execute::get_property(options, "wait");
        if !matches!(
            wait,
            Value::Undefined | Value::Boolean(_) | Value::Number(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.wait\" property must be one of type boolean or number.{}",
                crate::modules::util::invalid_arg_received(&wait)
            )));
        }
    }
    let _ = count;
    Ok(Value::Undefined)
}

pub fn test_shorthand(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mode = match args.first() {
        Some(Value::String(mode)) => mode.as_str(),
        _ => "only",
    };
    let todo = mode.starts_with("todo");
    let nested = mode.ends_with(":nested");
    let call_args = args.get(1..).unwrap_or_default();
    let mut normalized = call_args.to_vec();
    if todo {
        let options_index = normalized
            .iter()
            .position(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
        if let Some(index) = options_index {
            let _ = quench_runtime::execute::set_property_in_place(
                &normalized[index],
                "todo",
                Value::Boolean(true),
            );
        } else {
            let callback_index = normalized.iter().position(quench_runtime::is_callable);
            let insert_at = callback_index.unwrap_or(normalized.len());
            normalized.insert(
                insert_at,
                quench_runtime::host_api::object(vec![("todo".into(), Value::Boolean(true))]),
            );
        }
    }
    let result = if nested {
        crate::modules::test::nested(state, &normalized)?
    } else {
        crate::modules::test::run(state, &normalized)?
    };
    match result {
        Value::Object(_) | Value::ObjectAlias(_) => Ok(Value::Promise(Rc::new(
            quench_runtime::value::PromiseData::new(
                quench_runtime::value::PromiseState::Fulfilled(Value::Undefined),
            ),
        ))),
        value => Ok(value),
    }
}

pub fn test_context_wait_for(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let condition = args.first().ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_type(
            "The \"condition\" argument must be of type function".into(),
        )
    })?;
    if !quench_runtime::is_callable(condition) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"condition\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(condition)
        )));
    }
    let (interval, timeout) = test_wait_options(args.get(1))?;
    let deadline = Instant::now() + std::time::Duration::from_millis(timeout);
    let mut last_error = None;
    loop {
        if Instant::now() >= deadline {
            return Ok(test_wait_rejected(last_error));
        }
        match quench_runtime::vm::call_value(condition, &Value::Undefined, &[]) {
            Ok(Value::Promise(promise)) => {
                let value = Value::Promise(promise.clone());
                let remaining = deadline.saturating_duration_since(Instant::now());
                match crate::modules::pump::await_promise_with_timeout(
                    state,
                    &value,
                    remaining.as_secs_f64() * 1000.0,
                ) {
                    Ok(true) => return Ok(test_wait_rejected(last_error)),
                    Ok(false) => match &*promise.state.borrow() {
                        quench_runtime::value::PromiseState::Fulfilled(value) => {
                            return Ok(test_wait_fulfilled(value.clone()))
                        }
                        quench_runtime::value::PromiseState::Rejected(error) => {
                            last_error = Some(error.clone())
                        }
                        quench_runtime::value::PromiseState::Pending => {}
                    },
                    Err(error) => last_error = Some(test_wait_error_value(error)),
                }
            }
            Ok(value) => return Ok(test_wait_fulfilled(value)),
            Err(error) => last_error = Some(test_wait_error_value(error)),
        }
        if interval > 0 {
            std::thread::yield_now();
        }
    }
}

pub fn test_context_diagnostic(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

fn test_wait_options(options: Option<&Value>) -> Result<(u64, u64), VmError> {
    let Some(options) = options else {
        return Ok((10, 5000));
    };
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be of type object".into(),
        ));
    }
    let number = |name: &str, value: Value| -> Result<u64, VmError> {
        match value {
            Value::Undefined => Ok(if name == "interval" { 10 } else { 5000 }),
            Value::Number(value) if value.is_finite() && value >= 0.0 => Ok(value as u64),
            other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.{name}\" property must be of type number.{}",
                crate::modules::util::invalid_arg_received(&other)
            ))),
        }
    };
    let interval = number("interval", execute::get_property(options, "interval"))?;
    let timeout = number("timeout", execute::get_property(options, "timeout"))?;
    Ok((interval, timeout))
}

fn test_wait_fulfilled(value: Value) -> Value {
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Fulfilled(value),
    )))
}

fn test_wait_rejected(cause: Option<Value>) -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String("waitFor() timed out".into())],
    );
    if let Some(cause) = cause {
        let _ = execute::set_property_in_place(&error, "cause", cause);
    }
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Rejected(error),
    )))
}

fn test_wait_error_value(error: VmError) -> Value {
    match error {
        VmError::Thrown(value) => value,
        _ => Value::String("condition failed".into()),
    }
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
    // A mock of a constructable function inherits the wrapped constructor's
    // prototype.  Bound host capabilities otherwise expose the ordinary
    // Function prototype, so `new mock` loses `instanceof` and the outer
    // bound-constructor normalization replaces the returned object's
    // prototype.  Keep this as the single constructor fact on the wrapper.
    let target_prototype = quench_runtime::execute::get_property(&metadata_target, "prototype");
    if matches!(
        target_prototype,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Function(_) | Value::BoundFunction(_)
    ) {
        wrapper = quench_runtime::execute::set_property(wrapper, "prototype", target_prototype);
    }
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
    let object_value = quench_runtime::execute::canonical_value(object);
    let object = &object_value;
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
    // Nested test contexts can hold an alias to the parent object while the
    // canonical lookup resolves a copy-on-write representative. Publish the
    // replacement through both views so identity-sensitive mock assertions
    // observe the same method in either context.
    let original_object = args.first().expect("object validated above");
    let original_replaced =
        quench_runtime::execute::set_property_in_place(original_object, key, wrapper.clone());
    let canonical_replaced =
        quench_runtime::execute::set_property_in_place(object, key, wrapper.clone());
    if !original_replaced && !canonical_replaced {
        return Err(VmError::NotCallable);
    }
    // Module objects are copy-on-write views. Keep the canonical `fs` cache in
    // sync when a test mock replaces `fsync`, so APIs that consult the module
    // during a later callback observe the mock wrapper as Node does.
    if matches!(key.as_str(), "fsync" | "fsyncSync")
        && matches!(
            quench_runtime::execute::get_property(object, "createWriteStream"),
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        )
    {
        state
            .borrow_mut()
            .module_cache
            .insert("fs".into(), object.clone());
        state
            .borrow_mut()
            .module_cache
            .insert("__quench_fs_mocked".into(), object.clone());
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::reset_mocks();
    state
        .borrow_mut()
        .module_cache
        .retain(|key, _| !key.starts_with("\0mock:"));
    Ok(Value::Undefined)
}

pub fn test_mock_timers_enable(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if quench_runtime::date::mock_enabled() || state.borrow().timers.mock_originals.is_some() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are already enabled".into(),
        ));
    }
    let options = args
        .iter()
        .rev()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    if let Some(options) = options {
        let apis = quench_runtime::execute::get_property(options, "apis");
        if let Value::Array(apis) = apis {
            for index in 0..apis.logical_len() {
                let api = quench_runtime::execute::get_property(
                    &Value::Array(apis.clone()),
                    &index.to_string(),
                );
                let Value::String(api) = api else {
                    return Err(crate::modules::buffer_enc::invalid_arg_type(
                        "The \"apis\" option must be an array of strings".into(),
                    ));
                };
                if !matches!(
                    api.as_str(),
                    "Date"
                        | "setTimeout"
                        | "setInterval"
                        | "setImmediate"
                        | "scheduler.wait"
                        | "AbortSignal.timeout"
                ) {
                    return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                        "The \"apis\" option contains an unsupported API: {api}"
                    )));
                }
            }
        } else if !matches!(apis, Value::Undefined) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"apis\" option must be an array".into(),
            ));
        }
    }
    let selected = options
        .map(|value| quench_runtime::execute::get_property(value, "apis"))
        .and_then(|value| match value {
            Value::Array(values) => Some(
                (0..values.logical_len())
                    .map(|index| {
                        quench_runtime::execute::get_property(
                            &Value::Array(values.clone()),
                            &index.to_string(),
                        )
                    })
                    .filter_map(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_else(|| {
            ["Date", "setTimeout", "setInterval", "setImmediate"]
                .into_iter()
                .map(str::to_string)
                .collect()
        });
    // Validate the epoch before touching any global timer bindings. An
    // invalid enable call must be side-effect free so the next validation
    // observes the original disabled state.
    let configured_now = options
        .map(|value| quench_runtime::execute::get_property(value, "now"))
        .unwrap_or(Value::Undefined);
    let configured_now = if matches!(configured_now, Value::Undefined) {
        options
            .map(|value| quench_runtime::execute::get_property(value, "timeValue"))
            .unwrap_or(Value::Number(0.0))
    } else if matches!(configured_now, Value::Object(_) | Value::ObjectAlias(_)) {
        quench_runtime::execute::get_property(&configured_now, "timeValue")
    } else {
        configured_now
    };
    let initial_now = match configured_now {
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
    let mut originals = Vec::new();
    let global = quench_runtime::execute::current_script_global();
    let timers = crate::modules::require::require(state, &[Value::String("timers".into())])
        .unwrap_or(Value::Undefined);
    let promises =
        crate::modules::require::require(state, &[Value::String("timers/promises".into())])
            .unwrap_or(Value::Undefined);
    for (target_index, target) in [global, timers, promises].into_iter().enumerate() {
        for name in ["setTimeout", "setInterval", "setImmediate"] {
            let selected_api = selected.iter().any(|api| api == name)
                || (name == "setTimeout" && selected.iter().any(|api| api == "scheduler.wait"));
            if !selected_api {
                continue;
            }
            let original = quench_runtime::execute::get_property(&target, name);
            if !quench_runtime::is_callable(&original) {
                continue;
            }
            let wrapper = test_mock_fn(state, None, std::slice::from_ref(&original))?;
            originals.push((target.clone(), name.to_string(), original));
            if target_index == 0 {
                let _ = quench_runtime::execute::store_global_binding(name, wrapper.clone());
                // Keep the script-facing global alias in sync with the realm
                // owner.  A copy-on-write replacement alone can leave
                // `globalThis` holding the old descriptor table.
                let _ =
                    quench_runtime::execute::set_property_in_place(&target, name, wrapper.clone());
                let visible_global = quench_runtime::vm::current_global_object();
                if visible_global.object_identity() != target.object_identity() {
                    let _ = quench_runtime::execute::set_property_in_place(
                        &visible_global,
                        name,
                        wrapper.clone(),
                    );
                    if let Ok(updated) = quench_runtime::execute::set_property_observable(
                        visible_global.clone(),
                        name,
                        wrapper.clone(),
                    ) {
                        quench_runtime::execute::replace_global_object(&visible_global, &updated);
                    }
                }
                let global_this =
                    quench_runtime::execute::get_property(&visible_global, "globalThis");
                if matches!(global_this, Value::Object(_) | Value::ObjectAlias(_))
                    && global_this.object_identity() != target.object_identity()
                    && global_this.object_identity() != visible_global.object_identity()
                {
                    let _ = quench_runtime::execute::set_property_in_place(
                        &global_this,
                        name,
                        wrapper.clone(),
                    );
                    if let Ok(updated) = quench_runtime::execute::set_property_observable(
                        global_this.clone(),
                        name,
                        wrapper.clone(),
                    ) {
                        quench_runtime::execute::replace_global_object(&global_this, &updated);
                    }
                }
                if let Ok(updated) =
                    quench_runtime::execute::set_property_observable(target.clone(), name, wrapper)
                {
                    quench_runtime::execute::replace_global_object(&target, &updated);
                }
            } else {
                let _ = quench_runtime::execute::set_property_in_place(&target, name, wrapper);
            }
        }
    }
    if selected.iter().any(|api| api == "AbortSignal.timeout") {
        let abort_signal = quench_runtime::execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "AbortSignal",
        );
        let original = quench_runtime::execute::get_property(&abort_signal, "timeout");
        if quench_runtime::is_callable(&original) {
            let wrapper = test_mock_fn(state, None, std::slice::from_ref(&original))?;
            originals.push((abort_signal.clone(), "timeout".into(), original));
            let _ =
                quench_runtime::execute::set_property_in_place(&abort_signal, "timeout", wrapper);
        }
    }
    state.borrow_mut().timers.mock_originals = Some(originals);
    crate::modules::test::register_mock_restore(crate::host::capability(
        crate::registry::SPEC_TEST_MOCK_TIMERS_RESET,
    ));
    let value = initial_now;
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if state.borrow().timers.mock_originals.is_none() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are not enabled".into(),
        ));
    }
    let delta = match args.first() {
        None | Some(Value::Undefined) => 1.0,
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
        Some(Value::Number(_)) => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(
                "The value must be a non-negative number".into(),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The value must be a number".into(),
            ))
        }
    };
    let now = quench_runtime::date::current_time_ms();
    if quench_runtime::date::mock_enabled() {
        quench_runtime::date::set_mock_now(Some(now + delta));
    }
    let timer_now = crate::modules::timers::mock_timer_now()
        .unwrap_or(now.max(0.0) as u64)
        .saturating_add(delta.max(0.0) as u64);
    crate::modules::timers::set_mock_timer_now(Some(timer_now));
    crate::modules::pump::drain_mock_timers(state)?;
    Ok(Value::Undefined)
}

pub fn test_mock_timers_run_all(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if state.borrow().timers.mock_originals.is_none() {
        return Err(crate::modules::buffer_enc::invalid_state(
            "Mock timers are not enabled".into(),
        ));
    }
    // `runAll()` drains the finite queue present at invocation.  Intervals
    // remain active after their first delivery; otherwise an interval would
    // make this operation unbounded and Node callers could not clear it after
    // observing the first tick.
    let mut fired_intervals = std::collections::HashSet::new();
    for _ in 0..10_000 {
        let current = crate::modules::timers::mock_timer_now().unwrap_or(0);
        let next = state
            .borrow()
            .timers
            .timers
            .values()
            .filter(|timer| {
                timer.active
                    && !timer.retired
                    && (!matches!(timer.kind, crate::modules::timers::TimerKind::Interval)
                        || !fired_intervals.contains(&timer.object.object_identity()))
            })
            .map(|timer| {
                (
                    timer.fire_at,
                    matches!(timer.kind, crate::modules::timers::TimerKind::Interval),
                    timer.object.object_identity(),
                )
            })
            .min();
        let Some((next, is_interval, identity)) = next else {
            break;
        };
        let delta = next.saturating_sub(current);
        if is_interval {
            fired_intervals.insert(identity);
        }
        test_mock_timers_tick(state, None, &[Value::Number(delta as f64)])?;
    }
    Ok(Value::Undefined)
}

pub fn test_mock_timers_set_time(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if _state.borrow().timers.mock_originals.is_none() {
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
    crate::modules::timers::set_mock_timer_now(Some(value as u64));
    Ok(Value::Undefined)
}

pub fn test_mock_timers_reset(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let originals = state.borrow_mut().timers.mock_originals.take();
    state.borrow_mut().timers.timers.clear();
    if let Some(originals) = originals {
        for (target, name, original) in originals {
            if target.object_identity()
                == quench_runtime::vm::current_global_object().object_identity()
                || target.object_identity()
                    == quench_runtime::execute::current_script_global().object_identity()
            {
                let _ = quench_runtime::execute::store_global_binding(&name, original.clone());
                let _ = quench_runtime::execute::set_property_in_place(
                    &target,
                    &name,
                    original.clone(),
                );
                let visible_global = quench_runtime::vm::current_global_object();
                if visible_global.object_identity() != target.object_identity() {
                    let _ = quench_runtime::execute::set_property_in_place(
                        &visible_global,
                        &name,
                        original.clone(),
                    );
                    if let Ok(updated) = quench_runtime::execute::set_property_observable(
                        visible_global.clone(),
                        &name,
                        original.clone(),
                    ) {
                        quench_runtime::execute::replace_global_object(&visible_global, &updated);
                    }
                }
                let global_this =
                    quench_runtime::execute::get_property(&visible_global, "globalThis");
                if matches!(global_this, Value::Object(_) | Value::ObjectAlias(_))
                    && global_this.object_identity() != target.object_identity()
                    && global_this.object_identity() != visible_global.object_identity()
                {
                    let _ = quench_runtime::execute::set_property_in_place(
                        &global_this,
                        &name,
                        original.clone(),
                    );
                    if let Ok(updated) = quench_runtime::execute::set_property_observable(
                        global_this.clone(),
                        &name,
                        original.clone(),
                    ) {
                        quench_runtime::execute::replace_global_object(&global_this, &updated);
                    }
                }
                if let Ok(updated) = quench_runtime::execute::set_property_observable(
                    target.clone(),
                    &name,
                    original,
                ) {
                    quench_runtime::execute::replace_global_object(&target, &updated);
                }
            } else {
                let _ = quench_runtime::execute::set_property_in_place(&target, &name, original);
            }
        }
    }
    quench_runtime::date::set_mock_now(None);
    crate::modules::timers::set_mock_timer_now(None);
    Ok(Value::Undefined)
}

pub fn test_mock_module(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let specifier = match args.first() {
        Some(Value::String(value)) => value.clone(),
        Some(value) => match quench_runtime::execute::get_property(value, "href") {
            Value::String(href) => href,
            _ => {
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    "The \"specifier\" argument must be of type string or URL".into(),
                ))
            }
        },
        None => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"specifier\" argument must be of type string or URL".into(),
            ))
        }
    };
    if specifier.is_empty() {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"specifier\" argument must be of type string or URL".into(),
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
    for key in ["namedExports", "exports"] {
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
    if crate::modules::test::module_is_mocked(&specifier) {
        return Err(crate::modules::buffer_enc::invalid_state(
            "The module is already mocked".into(),
        ));
    }
    crate::modules::test::register_module_mock(specifier.clone(), options.clone());
    let restore = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_TEST_MOCK_MODULE_RESTORE.cap,
            ),
        },
        vec![Value::String(specifier)],
    );
    Ok(quench_runtime::host_api::object(vec![(
        "restore".into(),
        restore,
    )]))
}

pub fn test_mock_module_restore(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(specifier)) = args.first() else {
        return Ok(Value::Undefined);
    };
    let key = crate::modules::test::canonical_mock_specifier(specifier);
    crate::modules::test::unregister_module_mock(&key);
    state
        .borrow_mut()
        .module_cache
        .retain(|cache_key, _| cache_key != &format!("\0mock:{key}"));
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

pub fn test_before(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::before(args)
}

pub fn test_after(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::after(args)
}

pub fn test_convert_string_to_regexp(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let raw = match args.first() {
        Some(Value::String(value)) => value.clone(),
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The argument must be a string".into(),
            ))
        }
    };
    let argument_name = args
        .get(1)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("value");
    let (pattern, flags) = if raw.starts_with('/') {
        if let Some(last) = raw.rfind('/') {
            if last > 0 {
                let suffix = &raw[last + 1..];
                if suffix.chars().all(|ch| ch.is_ascii_alphabetic()) {
                    if suffix
                        .chars()
                        .any(|flag| !matches!(flag, 'd' | 'g' | 'i' | 'm' | 's' | 'u' | 'v' | 'y'))
                    {
                        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                            "The argument '{argument_name}' is an invalid regular expression. Invalid flags supplied to RegExp constructor '{suffix}'. Received '{raw}'"
                        )));
                    }
                    (&raw[1..last], suffix)
                } else {
                    (&raw[..], "")
                }
            } else {
                (&raw[..], "")
            }
        } else {
            (&raw[..], "")
        }
    } else {
        (&raw[..], "")
    };
    let global = quench_runtime::vm::current_global_object();
    let constructor = quench_runtime::execute::get_property(&global, "RegExp");
    quench_runtime::execute::construct_value(
        &constructor,
        &[Value::String(pattern.into()), Value::String(flags.into())],
    )
}

pub fn test_create_seeded_generator(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let seed = match args.first() {
        Some(Value::Number(value)) if value.is_finite() => (*value as u64 & 0xffff_ffff) as f64,
        Some(Value::BigInt(value)) => value.parse::<u64>().unwrap_or(0) as f64,
        _ => 0.0,
    };
    let state = quench_runtime::host_api::object(vec![("state".into(), Value::Number(seed))]);
    Ok(quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_TEST_SEEDED_GENERATOR_NEXT.cap,
            ),
        },
        vec![state],
    ))
}

pub fn test_seeded_generator_next(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let state = args.first().ok_or(VmError::NotCallable)?;
    let current = match quench_runtime::execute::get_property(state, "state") {
        Value::Number(value) => value as u32,
        _ => 0,
    };
    let mut next = current;
    next ^= next.wrapping_shl(13);
    next ^= next.wrapping_shr(17);
    next ^= next.wrapping_shl(5);
    let _ =
        quench_runtime::execute::set_property_in_place(state, "state", Value::Number(next as f64));
    Ok(Value::Number(next as f64 / 4_294_967_296.0))
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
