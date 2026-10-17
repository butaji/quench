use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn structured_clone(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(value) = args.first().copied() else {
        let error = context.type_error_rooted(
            "The \"The value argument must be specified\" argument must be specified",
        )?;
        let code = context.string_rooted("code");
        let missing_args = context.string_rooted("ERR_MISSING_ARGS");
        if !context.set_property_rooted(error, code, missing_args, error)? {
            return Err(RootedError::host("cannot set missing-argument error code"));
        }
        return Err(context.throw(error));
    };
    let options = args.get(1).copied().filter(|root| {
        context
            .rooted_value(*root)
            .is_some_and(|value| !value.is_undefined())
    });
    let transfers = if let Some(options) = options {
        let converters = context.with_startup_intrinsics(|context| {
            crate::host::shared_vm::commonjs::node_library_export(
                context,
                "internal/worker/js_transferable.js",
                "internal/webidl.js",
                "converters",
            )
        })?;
        let key = context.string_rooted("StructuredSerializeOptions");
        let converter = context.get_property_rooted(converters, key)?;
        let conversion = context.null_object_rooted()?;
        for (name, value) in [
            ("prefix", "Failed to execute 'structuredClone'"),
            ("context", "Options"),
        ] {
            let key = context.string_rooted(name);
            let value = context.string_rooted(value);
            context.define_data_property_rooted(conversion, key, value, true, true, true)?;
        }
        let undefined = context.undefined();
        let options = context.call_rooted(converter, undefined, &[options, conversion])?;
        let key = context.string_rooted("transfer");
        let array = context.get_property_rooted(options, key)?;
        let key = context.string_rooted("length");
        let length = context.get_property_rooted(array, key)?;
        let length = context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .ok_or_else(|| RootedError::host("Web IDL transfer sequence lost its array length"))?
            as usize;
        let mut transfers = Vec::with_capacity(length);
        for index in 0..length {
            let key = context.number(index as f64);
            transfers.push(context.get_property_rooted(array, key)?);
        }
        transfers
    } else {
        Vec::new()
    };
    context
        .structured_clone_with_transfers_rooted(value, &transfers)
        .map_err(|error| as_dom_exception(context, error))
}

/// A `DataCloneError` DOMException, as Node's serializer throws.
pub(crate) fn data_clone_exception(context: &mut NativeContext<'_, NodeHost>, message: &str) -> RootedError {
    let built = context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let key = context.string_rooted("DOMException");
        let constructor = context.get_property_rooted(global, key)?;
        let message = context.string_rooted(message);
        let name = context.string_rooted("DataCloneError");
        context.construct_rooted(constructor, constructor, &[message, name])
    });
    match built {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

/// One Node policy boundary for clone failures from structuredClone and Workers.
pub(crate) fn translate_clone_failure(
    context: &mut NativeContext<'_,NodeHost>,error:RootedError,
)->Result<RootedError,RootedError>{
    let Some(message)=error.error.data_clone_failure_message() else {return Ok(error)};
    let message=message.to_owned();
    if let Some(exception)=error.exception {context.release_root(exception);}
    let constructor=context.with_startup_intrinsics(|context| {
        let global=context.global_root()?;let key=context.string_rooted("DOMException");
        context.get_property_rooted(global,key)
    })?;
    let message=context.string_rooted(&message);let name=context.string_rooted("DataCloneError");
    let error=context.construct_rooted(constructor,constructor,&[message,name])?;
    Ok(context.throw(error))
}

/// Report the runtime's structured-serialization failures as Node's DOMException.
pub(crate) fn as_dom_exception(context: &mut NativeContext<'_, NodeHost>, error: RootedError) -> RootedError {
    let Some(message) = error.error.data_clone_failure_message().map(str::to_owned) else {
        return error;
    };
    if let Some(exception) = error.exception {
        context.release_root(exception);
    }
    data_clone_exception(context, &message)
}

/// Node's host marker is not a property and does not keep its object alive.
pub(crate) fn mark_as_uncloneable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if let Some(value) = args.first().copied() {
        validate_marker_target(context, value)?;
        context.mark_uncloneable_rooted(value)?;
    }
    Ok(context.undefined())
}

pub(crate) fn mark_as_untransferable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if let Some(value) = args.first().copied() {
        validate_marker_target(context, value)?;
        context.mark_untransferable_rooted(value)?;
    }
    Ok(context.undefined())
}

pub(crate) fn is_marked_as_untransferable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let marked = context.is_marked_untransferable_rooted(value)?;
    Ok(context.boolean(marked))
}

fn validate_marker_target(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<(), RootedError> {
    if context.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::Proxy)? {
        let error = context.type_error_rooted("Cannot pass private property name to proxy trap")?;
        return Err(context.throw(error));
    }
    Ok(())
}
