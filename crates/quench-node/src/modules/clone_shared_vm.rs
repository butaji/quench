use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn structured_clone(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(mut value) = args.first().copied() else {
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
    let is_file_backed_blob = if context.is_object_rooted(value)? {
        let file_blob_marker = context.string_rooted("\0quench:file-backed:clone-unsupported");
        let marker = context.get_property_rooted(value, file_blob_marker)?;
        context.truthy_rooted(marker)?
    } else {
        false
    };
    if is_file_backed_blob {
        let error =
            context.type_error_rooted("Invalid state: File-backed Blobs are not cloneable")?;
        let code = context.string_rooted("code");
        let value = context.string_rooted("ERR_INVALID_STATE");
        if !context.set_property_rooted(error, code, value, error)? {
            return Err(RootedError::host(
                "cannot set file-backed Blob clone error code",
            ));
        }
        return Err(context.throw(error));
    }
    // The public process.env surface uses a Proxy to match Node's assignment
    // coercion and deprecation behavior. Node's native environment object is
    // structured-cloneable, so turn that one proxy into an ordinary own-data
    // object before handing it to the VM serializer.
    let global = context.global_root()?;
    let process_key = context.string_rooted("process");
    let process = context.get_property_rooted(global, process_key)?;
    let env_key = context.string_rooted("env");
    let process_env = context.get_property_rooted(process, env_key)?;
    if context.strict_equal_rooted(value, process_env)? {
        let clone = context.object_rooted()?;
        for key in context.own_keys_rooted(value)? {
            let item = context.get_property_rooted(value, key)?;
            context.set_property_rooted(clone, key, item, clone)?;
        }
        value = clone;
    }
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
    #[cfg(quench_openssl_provider)]
    if transfers.is_empty() && context.is_object_rooted(value)? {
        // KeyObjects nested in a payload need the same transport records as
        // Worker messages. The direct same-realm clone path below only sees a
        // KeyObject at the root, so otherwise a nested KeyObject becomes an
        // empty ordinary object before MessagePort delivery.
        if contains_nested_key_object(context, value, &mut Vec::new())? {
            let prepared = crate::modules::crypto_key_shared_vm::prepare_key_objects_for_transport(
                context, value,
            )?;
            let cloned = match context.structured_clone_with_transfers_rooted(prepared, &[]) {
                Ok(cloned) => cloned,
                Err(error) => return Err(translate_clone_failure(context, error)?),
            };
            return crate::modules::crypto_key_shared_vm::revive_key_objects_after_transport(
                context, cloned,
            );
        }
    }
    if transfers.is_empty() && context.is_object_rooted(value)? {
        if let Some(cloned_key_object) =
            crate::modules::crypto_key_shared_vm::clone_key_object(context, value)?
        {
            return Ok(cloned_key_object);
        }
        if let Some(cloned_key) =
            crate::modules::crypto_key_shared_vm::clone_crypto_key(context, value)?
        {
            return Ok(cloned_key);
        }
    }
    if transfers.is_empty() {
        if let Some(clone) = clone_dom_exception(context, global, value)? {
            return Ok(clone);
        }
    }
    // The bootstrap Blob fallback is a JavaScript class, so the VM's native
    // structured-clone operation treats it as a plain object and drops its
    // prototype. Reconstruct direct Blob/File inputs before falling through
    // to the native clone path. File-backed blobs are rejected above.
    if transfers.is_empty() {
        if let Some(cloned_blob) = clone_blob_or_file(context, global, value)? {
            return Ok(cloned_blob);
        }
    }
    context
        .structured_clone_with_transfers_rooted(value, &transfers)
        .or_else(|error| Err(translate_clone_failure(context, error)?))
}

fn clone_dom_exception(
    context: &mut NativeContext<'_, NodeHost>,
    global: RootId,
    value: RootId,
) -> Result<Option<RootId>, RootedError> {
    if !context.is_object_rooted(value)? {
        return Ok(None);
    }
    let key = context.string_rooted("DOMException");
    let constructor = context.get_property_rooted(global, key)?;
    if !context.is_callable_rooted(constructor)?
        || !context.instance_of_rooted(value, constructor)?
    {
        return Ok(None);
    }

    let message_key = context.string_rooted("message");
    let message = context.get_property_rooted(value, message_key)?;
    let name_key = context.string_rooted("name");
    let name = context.get_property_rooted(value, name_key)?;
    let clone = context.construct_rooted(constructor, constructor, &[message, name])?;

    // The VM's generic Error clone path does not retain the DOMException
    // subclass or its captured stack. The DOMException constructor preserves
    // name/message/code; copy the original stack so repeated clones are stable.
    let stack_key = context.string_rooted("stack");
    let stack = context.get_property_rooted(value, stack_key)?;
    if context.string_text(stack)?.is_some()
        && !context.set_property_rooted(clone, stack_key, stack, clone)?
    {
        return Err(RootedError::host("cannot preserve DOMException stack"));
    }
    Ok(Some(clone))
}

#[cfg(quench_openssl_provider)]
fn contains_nested_key_object(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    seen: &mut Vec<RootId>,
) -> Result<bool, RootedError> {
    if !context.is_object_rooted(value)? {
        return Ok(false);
    }
    if let Some(clone) = crate::modules::crypto_key_shared_vm::clone_key_object(context, value)? {
        context.release_root(clone);
        return Ok(true);
    }
    if let Some(clone) = crate::modules::crypto_key_shared_vm::clone_crypto_key(context, value)? {
        context.release_root(clone);
        return Ok(true);
    }
    if context.clone_metadata_rooted(value)?.is_some()
        || context.builtin_data_rooted(value)?.is_some()
    {
        return Ok(false);
    }
    for previous in seen.iter() {
        if context.strict_equal_rooted(*previous, value)? {
            return Ok(false);
        }
    }
    seen.push(value);
    for key in context.enumerable_own_keys_rooted(value, false, true)? {
        let descriptor = context.own_property_descriptor_rooted(value, key)?;
        if !context.is_object_rooted(descriptor)? {
            continue;
        }
        let value_key = context.string_rooted("value");
        let field = context.own_property_descriptor_rooted(descriptor, value_key)?;
        if !context.is_object_rooted(field)? {
            continue;
        }
        let descriptor_value_key = context.string_rooted("value");
        let field_value = context.get_property_rooted(field, descriptor_value_key)?;
        context.release_root(descriptor_value_key);
        if contains_nested_key_object(context, field_value, seen)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn clone_blob_or_file(
    context: &mut NativeContext<'_, NodeHost>,
    global: RootId,
    value: RootId,
) -> Result<Option<RootId>, RootedError> {
    if !context.is_object_rooted(value)? {
        return Ok(None);
    }

    let file_key = context.string_rooted("File");
    let file_constructor = context.get_property_rooted(global, file_key)?;
    let is_file = !context
        .rooted_value(file_constructor)
        .is_some_and(|value| value.is_undefined())
        && context.instance_of_rooted(value, file_constructor)?;

    let blob_key = context.string_rooted("Blob");
    let blob_constructor = context.get_property_rooted(global, blob_key)?;
    let is_blob = !context
        .rooted_value(blob_constructor)
        .is_some_and(|value| value.is_undefined())
        && context.instance_of_rooted(value, blob_constructor)?;
    if !is_file && !is_blob {
        return Ok(None);
    }

    // Blob/File use a Node-side reconstruction path because the bootstrap
    // implementations are JavaScript classes. Validate them with the VM's
    // structured-clone operation first so host clone markers (notably
    // markAsUncloneable) are honored before that reconstruction bypasses the
    // normal serializer.
    let validation = context.structured_clone_with_transfers_rooted(value, &[]);
    match validation {
        Ok(clone) => {
            context.release_root(clone);
        }
        Err(error) => return Err(translate_clone_failure(context, error)?),
    }

    let data_key = context.string_rooted("_data");
    let data = context.get_property_rooted(value, data_key)?;
    let type_key = context.string_rooted("type");
    let mime_type = context.get_property_rooted(value, type_key)?;
    let parts = context.array_rooted(&[data])?;
    let options = context.object_rooted()?;
    let type_key = context.string_rooted("type");
    context.set_property_rooted(options, type_key, mime_type, options)?;

    let constructor = if is_file {
        let name_key = context.string_rooted("name");
        let name = context.get_property_rooted(value, name_key)?;
        let modified_key = context.string_rooted("lastModified");
        let modified = context.get_property_rooted(value, modified_key)?;
        let modified_key = context.string_rooted("lastModified");
        context.set_property_rooted(options, modified_key, modified, options)?;
        let args = [parts, name, options];
        return context
            .construct_rooted(file_constructor, file_constructor, &args)
            .map(Some);
    } else {
        blob_constructor
    };
    let args = [parts, options];
    context
        .construct_rooted(constructor, constructor, &args)
        .map(Some)
}

/// One Node policy boundary for clone failures from structuredClone and Workers.
pub(crate) fn translate_clone_failure(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootedError,
) -> Result<RootedError, RootedError> {
    let Some(message) = error.error.data_clone_failure_message() else {
        return Ok(error);
    };
    let message = message.to_owned();
    if let Some(exception) = error.exception {
        context.release_root(exception);
    }
    let constructor = context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let key = context.string_rooted("DOMException");
        context.get_property_rooted(global, key)
    })?;
    let message = context.string_rooted(&message);
    let name = context.string_rooted("DataCloneError");
    let error = context.construct_rooted(constructor, constructor, &[message, name])?;
    Ok(context.throw(error))
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
