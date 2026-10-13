//! Promise consumers own a private FIFO; the shared event handle owns kernel work.
use super::{get, nullish, set};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const DEFAULT_MAX_QUEUE: f64 = 2048.0;
const MAX_SAFE_INTEGER: f64 = ((1_u64 << f64::MANTISSA_DIGITS) - 1) as f64;
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(next, finish, PromiseConstructor, push, shift, ErrorConstructor) => {
 'use strict';
 class AbortError extends ErrorConstructor {}
 return { PromiseConstructor, push, shift, AbortError,
   makeIterator: state => ({ next: () => next(state), return: () => finish(state), [Symbol.asyncIterator]() { return this; } })
 };
}"#
);
pub(super) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    parent: RootId,
) -> Result<(), RootedError> {
    let next = context.host_function(crate::host::shared_vm::operation("fsPromiseWatchNext"))?;
    let finish =
        context.host_function(crate::host::shared_vm::operation("fsPromiseWatchReturn"))?;
    let global = context.global_root()?;
    let promise = get(context, global, "Promise")?;
    let array = get(context, global, "Array")?;
    let prototype = get(context, array, "prototype")?;
    let push = get(context, prototype, "push")?;
    let shift = get(context, prototype, "shift")?;
    let error = get(context, global, "Error")?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:fs/promise-watch-facade.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(
        factory,
        undefined,
        &[next, finish, promise, push, shift, error],
    )?;
    set(context, parent, "promise", api)
}
fn api(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let parent = shared
        .borrow()
        .fs_watch
        .api
        .ok_or_else(|| RootedError::host("watch API missing"))?;
    get(context, parent, "promise")
}
fn call(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = api(context)?;
    let function = get(context, api, name)?;
    context.call_rooted(function, receiver, args)
}
pub(super) fn abort_error(
    context: &mut NativeContext<'_, NodeHost>,
    signal: RootId,
) -> Result<RootId, RootedError> {
    let error = context.error_rooted("The operation was aborted")?;
    let api = api(context)?;
    let constructor = get(context, api, "AbortError")?;
    let prototype = get(context, constructor, "prototype")?;
    context.set_prototype_rooted(error, prototype)?;
    let cause = get(context, signal, "reason")?;
    let key = context.string_rooted("cause");
    context.define_data_property_rooted(error, key, cause, true, false, true)?;
    let code = context.string_rooted("ABORT_ERR");
    let name = context.string_rooted("AbortError");
    set(context, error, "code", code)?;
    set(context, error, "name", name)?;
    Ok(error)
}
fn option_boolean(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if context
        .rooted_value(value)
        .and_then(|value| value.as_bool())
        .is_none()
    {
        let error =
            crate::modules::util_shared_vm::argument_type_error(context, value, name, "boolean")?;
        return Err(crate::modules::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    }
    Ok(())
}
fn max_queue(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<(), RootedError> {
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        let error = crate::modules::util_shared_vm::argument_type_error(
            context,
            value,
            "options.maxQueue",
            "number",
        )?;
        return Err(crate::modules::net_options_shared_vm::validator_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    let rule = if !number.is_finite() || number.fract() != 0.0 {
        Some("an integer".to_owned())
    } else if number.abs() > MAX_SAFE_INTEGER {
        Some(format!(">= -{MAX_SAFE_INTEGER} && <= {MAX_SAFE_INTEGER}"))
    } else {
        None
    };
    if let Some(rule) = rule {
        let received = crate::modules::util_shared_vm::range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!("The value of \"options.maxQueue\" is out of range. It must be {rule}. Received {received}"))?;
        crate::modules::node_errors_shared_vm::decorate_range_error(
            context,
            error,
            "ERR_OUT_OF_RANGE",
        )?;
        crate::modules::node_errors_shared_vm::hide_range_error(
            context,
            error,
            "ERR_OUT_OF_RANGE",
        )?;
        return Err(context.throw(error));
    }
    Ok(())
}
pub(super) fn create(
    context: &mut NativeContext<'_, NodeHost>,
    filename: RootId,
    options: RootId,
) -> Result<RootId, RootedError> {
    let (_, filename) = super::path_argument(context, filename, "path")?;
    let saved = context.object_rooted()?;
    for name in [
        "persistent",
        "recursive",
        "encoding",
        "maxQueue",
        "overflow",
        "signal",
        "ignore",
    ] {
        let mut value = get(context, options, name)?;
        if context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            value = match name {
                "persistent" => context.boolean(true),
                "recursive" => context.boolean(false),
                "encoding" => context.string_rooted("utf8"),
                "maxQueue" => context.number(DEFAULT_MAX_QUEUE),
                "overflow" => context.string_rooted("ignore"),
                _ => value,
            };
        }
        set(context, saved, name, value)?;
    }
    for name in ["persistent", "recursive"] {
        let value = get(context, saved, name)?;
        option_boolean(context, value, &format!("options.{name}"))?;
    }
    let capacity = get(context, saved, "maxQueue")?;
    max_queue(context, capacity)?;
    let overflow = get(context, saved, "overflow")?;
    let overflow_text = context.string_text(overflow)?;
    if !matches!(overflow_text.as_deref(), Some("ignore" | "error")) {
        let inspect = crate::modules::util_shared_vm::internal_inspect(context)?;
        let undefined = context.undefined();
        let detail = context.call_rooted(inspect, undefined, &[overflow])?;
        let detail = context.to_string(detail)?;
        let error = context.type_error_rooted(&format!(
            "The property 'options.overflow' must be one of: 'ignore', 'error'. Received {detail}"
        ))?;
        crate::modules::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_INVALID_ARG_VALUE",
        )?;
        crate::modules::node_errors_shared_vm::hide_type_error(
            context,
            error,
            "ERR_INVALID_ARG_VALUE",
        )?;
        return Err(context.throw(error));
    }
    let signal = get(context, saved, "signal")?;
    crate::modules::util_shared_vm::validate_abort_signal(context, signal, "options.signal")?;
    let ignore = get(context, saved, "ignore")?;
    let matcher = super::ignore_matcher(context, ignore)?;
    let encoding = get(context, saved, "encoding")?;
    let encoding = super::validate_encoding(context, encoding)?;
    if !nullish(context, signal) {
        let aborted = get(context, signal, "aborted")?;
        if context.truthy_rooted(aborted)? {
            let error = abort_error(context, signal)?;
            return Err(context.throw(error));
        }
    }
    let data = context.object_rooted()?;
    let queue = context.array_rooted(&[])?;
    set(context, data, "queue", queue)?;
    set(context, data, "options", saved)?;
    set(context, data, "matcher", matcher)?;
    let phase = context.string_rooted("watching");
    set(context, data, "phase", phase)?;
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("fsPromiseWatchEvent"),
        data,
    )?;
    let persistent = get(context, saved, "persistent")?;
    let persistent = context.truthy_rooted(persistent)?;
    let handle = super::raw_watch(context, filename, persistent, encoding, callback)?;
    set(context, data, "handle", handle)?;
    if !nullish(context, signal) {
        let cancel = context.host_function_with_data(
            crate::host::shared_vm::operation("fsPromiseWatchCancel"),
            data,
        )?;
        set(context, data, "cancel", cancel)?;
        let add = get(context, signal, "addEventListener")?;
        let event = context.string_rooted("abort");
        let shared = context.host_mut().shared_state();
        let events = shared
            .borrow()
            .event_targets_api
            .ok_or_else(|| RootedError::host("event API missing"))?;
        let once = get(context, events, "abortListenerOptions")?;
        context.call_rooted(add, signal, &[event, cancel, once])?;
    }
    let undefined = context.undefined();
    call(context, "makeIterator", undefined, &[data])
}
fn queue_length(
    context: &mut NativeContext<'_, NodeHost>,
    queue: RootId,
) -> Result<f64, RootedError> {
    let length = get(context, queue, "length")?;
    context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("watch queue has invalid length"))
}
fn clear_queue(
    context: &mut NativeContext<'_, NodeHost>,
    queue: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted("length");
    let zero = context.number(0.0);
    context.set_property_rooted(queue, key, zero, queue)?;
    Ok(())
}
fn result(
    context: &mut NativeContext<'_, NodeHost>,
    item: Option<RootId>,
) -> Result<RootId, RootedError> {
    let result = context.null_object_rooted()?;
    if let Some(item) = item {
        set(context, result, "value", item)?;
    }
    let done = context.boolean(item.is_none());
    set(context, result, "done", done)?;
    Ok(result)
}
fn take(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
) -> Result<Option<(bool, RootId)>, RootedError> {
    let queue = get(context, data, "queue")?;
    if queue_length(context, queue)? == 0.0 {
        return Ok(None);
    }
    let record = call(context, "shift", queue, &[])?;
    let error = get(context, record, "error")?;
    if !nullish(context, error) {
        Ok(Some((true, error)))
    } else {
        let value = get(context, record, "value")?;
        Ok(Some((false, result(context, Some(value))?)))
    }
}
fn signal(context: &mut NativeContext<'_, NodeHost>, data: RootId) -> Result<RootId, RootedError> {
    let options = get(context, data, "options")?;
    get(context, options, "signal")
}
fn is_closed(context: &mut NativeContext<'_, NodeHost>, data: RootId) -> Result<bool, RootedError> {
    let phase = get(context, data, "phase")?;
    Ok(context.string_text(phase)?.as_deref() == Some("closed"))
}
fn settle(context: &mut NativeContext<'_, NodeHost>, data: RootId) -> Result<(), RootedError> {
    let pending = get(context, data, "pending")?;
    if nullish(context, pending) {
        return Ok(());
    }
    let outcome = if let Some(item) = take(context, data)? {
        Some(item)
    } else if is_closed(context, data)? {
        Some((false, result(context, None)?))
    } else {
        let signal = signal(context, data)?;
        if !nullish(context, signal) {
            let aborted = get(context, signal, "aborted")?;
            if context.truthy_rooted(aborted)? {
                Some((true, abort_error(context, signal)?))
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some((rejected, value)) = outcome {
        let function = get(
            context,
            pending,
            if rejected { "reject" } else { "resolve" },
        )?;
        let null = context.null();
        set(context, data, "pending", null)?;
        if rejected {
            let undefined = context.undefined();
            finish(context, undefined, &[data])?;
        }
        let undefined = context.undefined();
        context.call_rooted(function, undefined, &[value])?;
    }
    Ok(())
}
pub(crate) fn next(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = args.first().copied().unwrap_or_else(|| context.undefined());
    let executor = context.host_function_with_data(
        crate::host::shared_vm::operation("fsPromiseWatchExecutor"),
        data,
    )?;
    let api = api(context)?;
    let promise = get(context, api, "PromiseConstructor")?;
    context.construct_rooted(promise, promise, &[executor])
}
pub(crate) fn executor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let pending = context.object_rooted()?;
    let resolve = args.first().copied().unwrap_or_else(|| context.undefined());
    let reject = args.get(1).copied().unwrap_or_else(|| context.undefined());
    set(context, pending, "resolve", resolve)?;
    set(context, pending, "reject", reject)?;
    set(context, data, "pending", pending)?;
    settle(context, data)?;
    Ok(context.undefined())
}
pub(crate) fn event(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let kind = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let filename = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let matcher = get(context, data, "matcher")?;
    if !nullish(context, filename) && !nullish(context, matcher) {
        let undefined = context.undefined();
        let ignored = context.call_rooted(matcher, undefined, &[filename])?;
        if context.truthy_rooted(ignored)? {
            return Ok(undefined);
        }
    }
    let queue = get(context, data, "queue")?;
    let options = get(context, data, "options")?;
    let capacity = get(context, options, "maxQueue")?;
    let capacity = context
        .rooted_value(capacity)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("invalid watch queue limit"))?;
    let record = context.object_rooted()?;
    if queue_length(context, queue)? < capacity {
        let value = context.null_object_rooted()?;
        set(context, value, "eventType", kind)?;
        set(context, value, "filename", filename)?;
        set(context, record, "value", value)?;
        call(context, "push", queue, &[record])?;
        settle(context, data)?;
    } else {
        let overflow = get(context, options, "overflow")?;
        if context.string_text(overflow)?.as_deref() == Some("error") {
            clear_queue(context, queue)?;
            let error =
                context.error_rooted(&format!("fs.watch() queued more than {capacity} events"))?;
            let code = context.string_rooted("ERR_FS_WATCH_QUEUE_OVERFLOW");
            set(context, error, "code", code)?;
            set(context, record, "error", error)?;
            call(context, "push", queue, &[record])?;
            settle(context, data)?;
        } else {
            let global = context.global_root()?;
            let process = get(context, global, "process")?;
            let emit = get(context, process, "emitWarning")?;
            let text = context.string_rooted("fs.watch maxQueue exceeded");
            context.call_rooted(emit, process, &[text])?;
        }
    }
    Ok(context.undefined())
}
pub(crate) fn cancel(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let handle = get(context, data, "handle")?;
    super::handle_close(context, handle, &[handle])?;
    settle(context, data)?;
    Ok(context.undefined())
}
pub(crate) fn finish(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = args.first().copied().unwrap_or_else(|| context.undefined());
    if !is_closed(context, data)? {
        let phase = context.string_rooted("closed");
        set(context, data, "phase", phase)?;
        let handle = get(context, data, "handle")?;
        super::handle_close(context, handle, &[handle])?;
        let signal = signal(context, data)?;
        if !nullish(context, signal) {
            let cancel = get(context, data, "cancel")?;
            let remove = get(context, signal, "removeEventListener")?;
            let event = context.string_rooted("abort");
            context.call_rooted(remove, signal, &[event, cancel])?;
        }
        let queue = get(context, data, "queue")?;
        clear_queue(context, queue)?;
        settle(context, data)?;
    }
    result(context, None)
}
