use crate::host::NodeHost;
use crate::modules::event_loop::SharedCallback;
use rqj::{NativeContext, RootId, RootedError};
use std::time::Duration;

const IMMEDIATE_ID: &str = "\0quench:shared-immediate-id";
const TIMER_ID: &str = "\0quench:shared-timer-id";
const DEFAULT_TIMER_DELAY_MS: u64 = 1;
const MAX_TIMER_DELAY_MS: u64 = 2_147_483_647;

pub(crate) fn queue_microtask(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        return invalid_callback(context, "non-function");
    }
    context.queue_microtask_rooted(callback, &[])?;
    Ok(context.undefined())
}

pub(crate) fn set_timeout(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    schedule_timer(context, args, false)
}

pub(crate) fn set_interval(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    schedule_timer(context, args, true)
}

fn schedule_timer(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
    repeating: bool,
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        return invalid_callback(context, "non-function");
    }
    let delay = timer_delay(context, args.get(1).copied())?;
    let id = context
        .host_mut()
        .state()
        .borrow_mut()
        .event_loop
        .reserve_shared_timer_id()
        .ok_or_else(|| RootedError::host("shared timer identifier space exhausted"))?;
    let handle = context.object_rooted()?;
    let id_value = context.string_rooted(&id.to_string());
    let id_key = context.string_rooted(TIMER_ID);
    if !context.set_property_rooted(handle, id_key, id_value, handle)? {
        return Err(RootedError::host("cannot initialize Timeout handle"));
    }
    let callback = context.retain(callback)?;
    let receiver = context.retain(handle)?;
    let callback_args = args
        .get(2..)
        .unwrap_or_default()
        .iter()
        .copied()
        .map(|argument| context.retain(argument))
        .collect::<Result<Vec<_>, _>>()?;
    let callback = SharedCallback {
        callback,
        receiver,
        args: callback_args,
    };
    let interval = repeating.then_some(delay);
    context.host_mut().state().borrow_mut().event_loop.queue_shared_timer(
        id,
        delay,
        interval,
        callback,
    );
    Ok(handle)
}

fn timer_delay(
    context: &mut NativeContext<'_, NodeHost>,
    delay: Option<RootId>,
) -> Result<Duration, RootedError> {
    let Some(delay) = delay else {
        return Ok(Duration::from_millis(DEFAULT_TIMER_DELAY_MS));
    };
    let value = context
        .rooted_value(delay)
        .ok_or_else(|| RootedError::host("invalid timer delay root"))?;
    let numeric = if let Some(number) = value.as_number() {
        Some(number)
    } else if let Some(boolean) = value.as_bool() {
        Some(if boolean { 1.0 } else { 0.0 })
    } else if value.is_null() {
        Some(0.0)
    } else if let Some(text) = context.string_text(delay)? {
        if text.trim().is_empty() {
            Some(0.0)
        } else {
            text.trim().parse::<f64>().ok()
        }
    } else if value.is_undefined() {
        None
    } else {
        None
    };
    let milliseconds = numeric
        .filter(|value| value.is_finite())
        .map(|value| value.max(0.0).trunc() as u64)
        .unwrap_or(DEFAULT_TIMER_DELAY_MS)
        .clamp(DEFAULT_TIMER_DELAY_MS, MAX_TIMER_DELAY_MS);
    Ok(Duration::from_millis(milliseconds))
}

pub(crate) fn set_immediate(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_callback(context, "undefined");
    };
    if !context.is_callable_rooted(callback)? {
        let received = context.rooted_value(callback).map_or("undefined", |value| {
            if value.is_undefined() {
                "undefined"
            } else if value.is_null() {
                "null"
            } else if value.as_number().is_some() {
                "a number"
            } else if value.as_bool().is_some() {
                "a boolean"
            } else {
                "an instance of Object"
            }
        });
        return invalid_callback(context, received);
    }

    let id = context
        .host_mut()
        .state()
        .borrow_mut()
        .event_loop
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("shared Immediate identifier space exhausted"))?;
    let handle = context.object_rooted()?;
    let id_value = context.number(id as f64);
    let id_key = context.string_rooted(IMMEDIATE_ID);
    if !context.set_property_rooted(handle, id_key, id_value, handle)? {
        return Err(RootedError::host("cannot initialize Immediate handle"));
    }
    let callback = context.retain(callback)?;
    let receiver = context.retain(handle)?;
    let args = args[1..]
        .iter()
        .copied()
        .map(|argument| context.retain(argument))
        .collect::<Result<Vec<_>, _>>()?;
    context
        .host_mut()
        .state()
        .borrow_mut()
        .event_loop
        .queue_shared_immediate(id, SharedCallback {
            callback,
            receiver,
            args,
        });
    Ok(handle)
}

pub(crate) fn clear_immediate(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(handle) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let key = context.string_rooted(IMMEDIATE_ID);
    let id = context
        .get_property_rooted(handle, key)
        .ok()
        .and_then(|value| context.rooted_value(value))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
        .map(|value| value as u64);
    if let Some(id) = id {
        if let Some(callback) = context
            .host_mut()
            .state()
            .borrow_mut()
            .event_loop
            .cancel_shared_immediate(id)
        {
            release_callback(context, callback);
        }
    }
    Ok(context.undefined())
}

pub(crate) fn clear_timer(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(handle) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let key = context.string_rooted(TIMER_ID);
    let id = context
        .get_property_rooted(handle, key)
        .ok()
        .and_then(|value| context.string_text(value).ok().flatten())
        .and_then(|value| value.parse::<u64>().ok());
    if let Some(id) = id {
        let callback = context
            .host_mut()
            .state()
            .borrow_mut()
            .event_loop
            .cancel_shared_timer(id);
        if let Some(callback) = callback {
            release_callback(context, callback);
        }
    }
    Ok(context.undefined())
}

fn invalid_callback(
    context: &mut NativeContext<'_, NodeHost>,
    received: &str,
) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"callback\" argument must be of type function. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let property = context.string_rooted("code");
    if !context.set_property_rooted(error, property, code, error)? {
        return Err(RootedError::host("cannot set setImmediate error code"));
    }
    Err(context.throw(error))
}

fn release_callback(
    context: &mut NativeContext<'_, NodeHost>,
    callback: SharedCallback,
) {
    context.release_root(callback.callback);
    context.release_root(callback.receiver);
    for argument in callback.args {
        context.release_root(argument);
    }
}

pub(crate) fn release_runtime_callback(
    runtime: &mut rqj::Runtime<NodeHost>,
    callback: SharedCallback,
) {
    runtime.release_root(callback.callback);
    runtime.release_root(callback.receiver);
    for argument in callback.args {
        runtime.release_root(argument);
    }
}

pub(crate) fn timer_callback(
    runtime: &mut rqj::Runtime<NodeHost>,
    program: &rqj::ResidualProgram,
    callback: &SharedCallback,
) -> Result<(), String> {
    match runtime.call_rooted(callback.callback, callback.receiver, &callback.args) {
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
