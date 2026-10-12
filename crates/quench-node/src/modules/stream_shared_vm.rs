//! Shared-VM projection of the existing guest stream state machine.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    string_decoder: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(
        crate::modules::stream_source::PRELUDE,
        "node:stream/shared.js",
    )?;
    let dependencies = context.object_rooted()?;
    let pipe_plan = context.host_function(crate::host::shared_vm::operation("streamPipePlan"))?;
    set(context, dependencies, "pipePlan", pipe_plan)?;
    let pipe_end =
        context.host_function(crate::host::shared_vm::operation("streamPipeShouldEnd"))?;
    set(context, dependencies, "pipeShouldEnd", pipe_end)?;
    let apply_options = context.host_function(crate::host::shared_vm::operation(
        "streamApplyTransformOptions",
    ))?;
    set(
        context,
        dependencies,
        "applyTransformOptions",
        apply_options,
    )?;
    let default_hwm = context.host_function(crate::host::shared_vm::operation(
        "streamGetDefaultHighWaterMark",
    ))?;
    set(
        context,
        dependencies,
        "getDefaultHighWaterMark",
        default_hwm,
    )?;
    let replay_state = context.null_object_rooted()?;
    let replay = context.host_function_with_data(
        crate::host::shared_vm::operation("streamReplayWrite"),
        replay_state,
    )?;
    set(context, dependencies, "replayWrite", replay)?;
    let events = context.object_rooted()?;
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    set(context, events, "EventEmitter", emitter)?;
    set(context, dependencies, "events", events)?;

    set(context, dependencies, "string_decoder", string_decoder)?;

    let undefined = context.undefined();
    let module = context.call_rooted(factory, undefined, &[dependencies])?;
    let key = context.string_rooted("Writable");
    let writable = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(writable, key)?;
    let key = context.string_rooted("write");
    let write = context.get_property_rooted(prototype, key)?;
    set(context, replay_state, "write", write)?;
    for (name, operation, factory) in [
        (
            "getDefaultHighWaterMark",
            "streamGetDefaultHighWaterMark",
            "operation => function getDefaultHighWaterMark(objectMode) { return operation(objectMode); }",
        ),
        (
            "setDefaultHighWaterMark",
            "streamSetDefaultHighWaterMark",
            "operation => function setDefaultHighWaterMark(objectMode, value) { return operation(objectMode, value); }",
        ),
    ] {
        let operation = context.host_function(crate::host::shared_vm::operation(operation))?;
        let factory =
            context.evaluate_script_rooted(factory, "node:stream/default-high-water-mark.js")?;
        let undefined = context.undefined();
        let function = context.call_rooted(factory, undefined, &[operation])?;
        set(context, module, name, function)?;
    }
    let key = context.string_rooted("Readable");
    let readable = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(readable, key)?;
    let getter = context.host_function(crate::host::shared_vm::operation("readableEncoding"))?;
    let name = context.string_rooted("name");
    let get_name = context.string_rooted("get");
    context.define_data_property_rooted(getter, name, get_name, false, false, true)?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    let key = context.string_rooted("readableEncoding");
    context.define_property_descriptor_rooted(prototype, key, descriptor)?;
    Ok(module)
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
            "cannot install shared stream dependency {name}"
        )))
    }
}

pub(crate) fn readable_encoding(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let key = context.string_rooted("_readableState");
    let state = context.get_property_rooted(receiver, key)?;
    if !context.truthy_rooted(state)? {
        return Ok(context.null());
    }
    let key = context.string_rooted("encoding");
    context.get_property_rooted(state, key)
}

/// A queued chunk already belongs to the writable buffered total. Replay via
/// the captured intrinsic without admitting or accounting for it a second time.
pub(crate) fn replay_write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let key = context.string_rooted("write");
    let write = context.get_property_rooted(data, key)?;
    let undefined = context.undefined();
    let values = [
        args.get(1).copied().unwrap_or(undefined),
        args.get(2).copied().unwrap_or(undefined),
        args.get(3).copied().unwrap_or(undefined),
        context.boolean(true),
    ];
    context.call_rooted(write, args[0], &values)
}

pub(crate) fn get_default_high_water_mark(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let object_mode = context.truthy_rooted(args.first().copied().unwrap_or(undefined))?;
    let value = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        if object_mode {
            state.stream_high_water_marks.objects
        } else {
            state.stream_high_water_marks.bytes
        }
    };
    Ok(context.number(value))
}

pub(crate) fn set_default_high_water_mark(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let value = args.get(1).copied().unwrap_or(undefined);
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context, value, "value", "number",
        )?);
    };
    let constraint = if !number.is_finite() || number.fract() != 0.0 {
        Some("an integer".to_owned())
    } else if !(0.0..=super::stream_state::MAX_SAFE_HIGH_WATER_MARK).contains(&number) {
        Some(format!(
            ">= 0 && <= {}",
            super::stream_state::MAX_SAFE_HIGH_WATER_MARK
        ))
    } else {
        None
    };
    if let Some(constraint) = constraint {
        let received = context.to_string(value)?;
        let error = context.range_error_rooted(&format!(
            "The value of \"value\" is out of range. It must be {constraint}. Received {received}"
        ))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        context.set_property_rooted(error, key, code, error)?;
        return Err(context.throw(error));
    }
    let object_mode = context.truthy_rooted(args.first().copied().unwrap_or(undefined))?;
    {
        let state = context.host_mut().shared_state();
        let mut state = state.borrow_mut();
        if object_mode {
            state.stream_high_water_marks.objects = number;
        } else {
            state.stream_high_water_marks.bytes = number;
        }
    }
    Ok(undefined)
}

pub(crate) fn apply_transform_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let options = args.get(1).copied().unwrap_or(undefined);
    if !context.truthy_rooted(options)? {
        return Ok(undefined);
    }
    let stream = *args
        .first()
        .ok_or_else(|| RootedError::host("Transform receiver missing"))?;
    for (option, method) in [("transform", "_transform"), ("flush", "_flush")] {
        let key = context.string_rooted(option);
        let value = context.get_property_rooted(options, key)?;
        if context.is_callable_rooted(value)? {
            let value = context.get_property_rooted(options, key)?;
            set(context, stream, method, value)?;
        }
    }
    Ok(undefined)
}

pub(crate) fn pipe_should_end(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let destination = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("pipe destination missing"))?;
    if let Some(options) = args
        .get(1)
        .copied()
        .filter(|options| context.truthy_rooted(*options).unwrap_or(false))
    {
        let key = context.string_rooted("end");
        let end = context.get_property_rooted(options, key)?;
        let false_value = context.boolean(false);
        if context.strict_equal_rooted(end, false_value)? {
            return Ok(false_value);
        }
    }
    for name in ["stdout", "stderr"] {
        let global = context.global_root()?;
        let key = context.string_rooted("process");
        let process = context.get_property_rooted(global, key)?;
        let key = context.string_rooted(name);
        let stdio = context.get_property_rooted(process, key)?;
        if context.strict_equal_rooted(destination, stdio)? {
            return Ok(context.boolean(false));
        }
    }
    Ok(context.boolean(true))
}
