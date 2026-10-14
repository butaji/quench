//! Console counter/timer transitions and their trace events share one state.
use super::event_constructor_shared_vm::get;
use super::trace_state::Phase;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use serde_json::json;

const DEFAULT_LABEL: &str = "default";
const MILLISECONDS_PER_SECOND: f64 = 1000.0;
const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;
const SECONDS_PER_MINUTE: f64 = 60.0;
const MINUTES_PER_HOUR: f64 = 60.0;
const WRAPPERS: &str = r#"(api => ({
  time: function time(label = 'default') { return api.time(this, label); },
  timeEnd: function timeEnd(label = 'default') { return api.timeEnd(this, label); },
  timeLog: function timeLog(label = 'default', ...args) { return api.timeLog(this, label, ...args); },
  count: function count(label = 'default') { return api.count(this, label); },
  countReset: function countReset(label = 'default') { return api.countReset(this, label); }
}))"#;
fn put(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.define_data_property_rooted(object, key, value, true, true, true)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot define native state {name}"
        )))
    }
}
fn invoke(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = get(context, object, name)?;
    context.call_rooted(method, object, args)
}
fn captured(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = context.host_function_data()?;
    let function = get(context, api, name)?;
    context.call_rooted(function, receiver, args)
}
fn absent(context: &NativeContext<'_, NodeHost>, root: RootId) -> bool {
    context.rooted_value(root).is_some_and(|v| v.is_undefined())
}
fn label(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<String, RootedError> {
    if absent(context, value) {
        return Ok(DEFAULT_LABEL.into());
    }
    // Abstract ToString, unlike the explicitly callable String constructor,
    // rejects Symbols and retains coercion exceptions.
    context.to_string(value)
}
fn warning(context: &mut NativeContext<'_, NodeHost>, text: &str) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let text = context.string_rooted(text);
    invoke(context, process, "emitWarning", &[text])?;
    Ok(())
}
fn counts(
    context: &mut NativeContext<'_, NodeHost>,
    console: RootId,
) -> Result<RootId, RootedError> {
    let api = context.host_function_data()?;
    let states = get(context, api, "states")?;
    let map = context.weak_map_get_rooted(states, console)?;
    if !absent(context, map) {
        return Ok(map);
    }
    let ctor = get(context, api, "Map")?;
    let map = context.construct_rooted(ctor, ctor, &[])?;
    let setter = get(context, api, "weakSet")?;
    context.call_rooted(setter, states, &[console, map])?;
    Ok(map)
}
fn times(
    context: &mut NativeContext<'_, NodeHost>,
    console: RootId,
) -> Result<RootId, RootedError> {
    get(context, console, "_times")
}
pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    prototype: RootId,
) -> Result<(), RootedError> {
    let api = context.null_object_rooted()?;
    let global = context.global_root()?;
    let map = get(context, global, "Map")?;
    put(context, api, "Map", map)?;
    let map_prototype = get(context, map, "prototype")?;
    for (name, method) in [
        ("mapGet", "get"),
        ("mapSet", "set"),
        ("mapHas", "has"),
        ("mapDelete", "delete"),
    ] {
        let function = get(context, map_prototype, method)?;
        put(context, api, name, function)?;
    }
    let weak = get(context, global, "WeakMap")?;
    let weak_prototype = get(context, weak, "prototype")?;
    let setter = get(context, weak_prototype, "set")?;
    let states = context.construct_rooted(weak, weak, &[])?;
    put(context, api, "states", states)?;
    put(context, api, "weakSet", setter)?;
    for (name, operation) in [
        ("time", "consoleTime"),
        ("timeEnd", "consoleTimeEnd"),
        ("timeLog", "consoleTimeLog"),
        ("count", "consoleCount"),
        ("countReset", "consoleCountReset"),
    ] {
        let function =
            context.host_function_with_data(crate::host::shared_vm::operation(operation), api)?;
        put(context, api, name, function)?;
    }
    let factory = context.evaluate_script_rooted(WRAPPERS, "node:console/counter-timer-methods")?;
    let undefined = context.undefined();
    let methods = context.call_rooted(factory, undefined, &[api])?;
    for name in ["time", "timeEnd", "timeLog", "count", "countReset"] {
        let function = get(context, methods, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    Ok(())
}
pub(crate) fn count(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let console = args.first().copied().unwrap_or(absent);
    let text = label(context, args.get(1).copied().unwrap_or(absent))?;
    let label = context.string_rooted(&text);
    let map = counts(context, console)?;
    let previous = captured(context, "mapGet", map, &[label])?;
    let count = if super::console_timing_shared_vm::absent(context, previous) {
        context.number(1.0)
    } else {
        context.numeric_update_rooted(previous, false)?
    };
    captured(context, "mapSet", map, &[label, count])?;
    let number = context
        .rooted_value(count)
        .and_then(|v| v.as_number())
        .unwrap_or(0.0);
    context.host_mut().shared_state().borrow_mut().trace.record(
        "node,node.console",
        &format!("count::{text}"),
        Phase::Counter,
        Some(0),
        json!({"data":number as u64}),
    );
    let count_text = context.to_string(count)?;
    let output = context.string_rooted(&format!("{text}: {count_text}"));
    invoke(context, console, "log", &[output])?;
    Ok(absent)
}
pub(crate) fn count_reset(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let console = args.first().copied().unwrap_or(absent);
    let raw = args.get(1).copied().unwrap_or(absent);
    let raw = if super::console_timing_shared_vm::absent(context, raw) {
        context.string_rooted(DEFAULT_LABEL)
    } else {
        raw
    };
    let map = counts(context, console)?;
    let has = captured(context, "mapHas", map, &[raw])?;
    let text = label(context, raw)?;
    if !context.truthy_rooted(has)? {
        warning(context, &format!("Count for '{text}' does not exist"))?;
        return Ok(absent);
    }
    context.host_mut().shared_state().borrow_mut().trace.record(
        "node,node.console",
        &format!("count::{text}"),
        Phase::Counter,
        Some(0),
        json!({"data":0}),
    );
    let label = context.string_rooted(&text);
    captured(context, "mapDelete", map, &[label])?;
    Ok(absent)
}
pub(crate) fn time(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let console = args.first().copied().unwrap_or(absent);
    let map = times(context, console)?;
    let raw_label = args.get(1).copied().unwrap_or(absent);
    let trace_label = label(context, raw_label)?;
    let text = label(context, raw_label)?;
    let label = context.string_rooted(&text);
    let has = captured(context, "mapHas", map, &[label])?;
    if context.truthy_rooted(has)? {
        warning(
            context,
            &format!("Label '{text}' already exists for console.time()"),
        )?;
        return Ok(absent);
    }
    context.host_mut().shared_state().borrow_mut().trace.record(
        "node,node.console",
        &format!("time::{trace_label}"),
        Phase::AsyncBegin,
        Some(0),
        json!({}),
    );
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let start = invoke(context, process, "hrtime", &[])?;
    captured(context, "mapSet", map, &[label, start])?;
    Ok(absent)
}
fn format_time(milliseconds: f64) -> String {
    let minutes = (milliseconds / (MILLISECONDS_PER_SECOND * SECONDS_PER_MINUTE)).floor();
    if minutes > 0.0 {
        let hours = (minutes / MINUTES_PER_HOUR).floor();
        let seconds = (milliseconds / MILLISECONDS_PER_SECOND) % SECONDS_PER_MINUTE;
        let seconds = format!("{seconds:06.3}");
        if hours > 0.0 {
            format!(
                "{hours}:{:02.0}:{seconds} (h:mm:ss.mmm)",
                minutes % MINUTES_PER_HOUR
            )
        } else {
            format!("{minutes}:{seconds} (m:ss.mmm)")
        }
    } else if milliseconds >= MILLISECONDS_PER_SECOND {
        format!("{:.3}s", milliseconds / MILLISECONDS_PER_SECOND)
    } else {
        let rounded = format!("{milliseconds:.3}");
        format!("{}ms", rounded.trim_end_matches('0').trim_end_matches('.'))
    }
}
fn log_time(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    end: bool,
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let console = args.first().copied().unwrap_or(absent);
    let map = times(context, console)?;
    let raw_label = args.get(1).copied().unwrap_or(absent);
    let trace_label = label(context, raw_label)?;
    let text = label(context, raw_label)?;
    let label = context.string_rooted(&text);
    let start = captured(context, "mapGet", map, &[label])?;
    if super::console_timing_shared_vm::absent(context, start) {
        warning(
            context,
            &format!(
                "No such label '{text}' for console.{}()",
                if end { "timeEnd" } else { "timeLog" }
            ),
        )?;
    } else {
        let global = context.global_root()?;
        let process = get(context, global, "process")?;
        let duration = invoke(context, process, "hrtime", &[start])?;
        let seconds = get(context, duration, "0")?;
        let nanoseconds = get(context, duration, "1")?;
        let milliseconds = context
            .rooted_value(seconds)
            .and_then(|v| v.as_number())
            .unwrap_or(0.0)
            * MILLISECONDS_PER_SECOND
            + context
                .rooted_value(nanoseconds)
                .and_then(|v| v.as_number())
                .unwrap_or(0.0)
                / NANOSECONDS_PER_MILLISECOND;
        let pattern = context.string_rooted("%s: %s");
        let formatted = context.string_rooted(&format_time(milliseconds));
        let mut output = vec![pattern, label, formatted];
        if !end {
            output.extend_from_slice(args.get(2..).unwrap_or(&[]));
        }
        invoke(context, console, "log", &output)?;
    }
    context.host_mut().shared_state().borrow_mut().trace.record(
        "node,node.console",
        &format!("time::{trace_label}"),
        if end {
            Phase::AsyncEnd
        } else {
            Phase::AsyncInstant
        },
        Some(0),
        json!({}),
    );
    if end {
        captured(context, "mapDelete", map, &[label])?;
    }
    Ok(absent)
}
pub(crate) fn time_end(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    log_time(context, args, true)
}
pub(crate) fn time_log(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    log_time(context, args, false)
}
