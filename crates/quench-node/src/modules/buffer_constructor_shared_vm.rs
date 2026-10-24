//! Legacy Buffer construction is ordinary native Node policy, not a class call.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const FAST_BUFFER_CACHE: &str = "\0builtin:buffer/FastBuffer";
const MAX_DEPRECATION_LOCATION_CHECKS: f64 = 10_000.0;
const WARNING: &str = "Buffer() is deprecated due to security and usability issues. Please use the Buffer.alloc(), Buffer.allocUnsafe(), or Buffer.from() methods instead.";
fn get(context: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn set(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}
fn name(context: &mut Context<'_>, function: RootId, name: &str) -> Result<(), RootedError> {
    let key = context.string_rooted("name");
    let value = context.string_rooted(name);
    context.define_data_property_rooted(function, key, value, false, false, true)?;
    Ok(())
}
pub(crate) fn install(context: &mut Context<'_>, fast: RootId) -> Result<RootId, RootedError> {
    let data = context.object_rooted()?;
    let no = context.boolean(false);
    set(context, data, "warned", no)?;
    let zero = context.number(0.0);
    set(context, data, "checks", zero)?;
    let pending = context
        .host_mut()
        .shared_state()
        .borrow()
        .cli_options
        .enabled("--pending-deprecation");
    let pending = context.boolean(pending);
    set(context, data, "pending", pending)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    set(context, data, "process", process)?;
    let constructor = context.host_constructor_with_data(
        crate::host::shared_vm::operation("bufferLegacyConstruct"),
        data,
    )?;
    name(context, constructor, "Buffer")?;
    let pool_size = context.number(super::buffer_pool_shared_vm::DEFAULT_POOL_SIZE as f64);
    set(context, constructor, "poolSize", pool_size)?;
    set(context, data, "constructor", constructor)?;
    for key in context.own_keys_rooted(fast)? {
        if context
            .string_text(key)?
            .is_some_and(|name| matches!(name.as_str(), "name" | "length" | "prototype"))
        {
            continue;
        }
        let descriptor = context.own_property_descriptor_rooted(fast, key)?;
        context.define_property_descriptor_rooted(constructor, key, descriptor)?;
    }
    let parent = context.prototype_rooted(fast)?;
    context.set_prototype_rooted(constructor, parent)?;
    let prototype = get(context, fast, "prototype")?;
    let key = context.string_rooted("prototype");
    context.define_data_property_rooted(constructor, key, prototype, true, false, false)?;
    set(context, prototype, "constructor", constructor)?;
    name(context, fast, "FastBuffer")?;
    let retained = context.retain(fast)?;
    let previous = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(FAST_BUFFER_CACHE.to_owned(), retained);
    if let Some(previous) = previous {
        context.release_root(previous);
    }
    let is_buffer = context.host_constructor_with_data(
        crate::host::shared_vm::operation("bufferIsBuffer"),
        constructor,
    )?;
    name(context, is_buffer, "isBuffer")?;
    let key = context.string_rooted("isBuffer");
    context.define_data_property_rooted(constructor, key, is_buffer, true, true, true)?;
    let of =
        context.host_function_with_data(crate::host::shared_vm::operation("bufferOf"), fast)?;
    name(context, of, "of")?;
    let key = context.string_rooted("of");
    context.define_data_property_rooted(constructor, key, of, true, true, true)?;
    let symbol = get(context, global, "Symbol")?;
    let species = get(context, symbol, "species")?;
    let getter = context
        .host_function_with_data(crate::host::shared_vm::operation("bufferSpeciesGet"), fast)?;
    name(context, getter, "get")?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    let yes = context.boolean(true);
    set(context, descriptor, "configurable", yes)?;
    context.define_property_descriptor_rooted(constructor, species, descriptor)?;
    super::buffer_from_shared_vm::install(context, constructor, fast)?;
    Ok(constructor)
}
fn warn(context: &mut Context<'_>, data: RootId) -> Result<(), RootedError> {
    let warned = get(context, data, "warned")?;
    if context.truthy_rooted(warned)? {
        return Ok(());
    }
    let checks = get(context, data, "checks")?;
    let checks = context
        .rooted_value(checks)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("Buffer deprecation counter missing"))?
        + 1.0;
    let value = context.number(checks);
    set(context, data, "checks", value)?;
    if checks > MAX_DEPRECATION_LOCATION_CHECKS {
        return Ok(());
    }
    let pending = get(context, data, "pending")?;
    if !context.truthy_rooted(pending)? {
        // Node suppresses this warning for the nearest non-internal module.
        // Source names are compiler metadata, never parsed source or fixture IDs.
        if let Some(source) = context
            .call_stack_source_names()
            .into_iter()
            .find(|source| !source.starts_with("node:") && !source.starts_with('<'))
        {
            if source
                .rsplit(['/', '\\'])
                .skip(1)
                .any(|component| component == "node_modules")
            {
                return Ok(());
            }
        }
    }
    let process = get(context, data, "process")?;
    let emit = get(context, process, "emitWarning")?;
    let message = context.string_rooted(WARNING);
    let kind = context.string_rooted("DeprecationWarning");
    let code = context.string_rooted("DEP0005");
    context.call_rooted(emit, process, &[message, kind, code])?;
    let yes = context.boolean(true);
    set(context, data, "warned", yes)?;
    Ok(())
}
pub(crate) fn construct(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    warn(context, data)?;
    let undefined = context.undefined();
    let first = args.first().copied().unwrap_or(undefined);
    let encoding = args.get(1).copied().unwrap_or(undefined);
    let length = args.get(2).copied().unwrap_or(undefined);
    let constructor = get(context, data, "constructor")?;
    if context
        .rooted_value(first)
        .and_then(|value| value.as_number())
        .is_some()
    {
        if context.string_text(encoding)?.is_some() {
            return Err(super::util_shared_vm::argument_type_error(
                context, first, "string", "string",
            )?);
        }
        let alloc = get(context, constructor, "alloc")?;
        context.call_rooted(alloc, constructor, &[first])
    } else {
        let from = get(context, constructor, "from")?;
        context.call_rooted(from, constructor, &[first, encoding, length])
    }
}
pub(crate) fn is_buffer(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let constructor = context.host_function_data()?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let yes = context.instance_of_rooted(value, constructor)?;
    Ok(context.boolean(yes))
}
pub(crate) fn species(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.host_function_data()
}

/// Node's own `of` allocates through FastBuffer without invoking legacy warnings.
pub(crate) fn of(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fast = context.host_function_data()?;
    let length = context.number(args.len() as f64);
    let result = context.construct_rooted(fast, fast, &[length])?;
    for (index, value) in args.iter().copied().enumerate() {
        let key = context.string_rooted(&index.to_string());
        context.set_property_rooted(result, key, value, result)?;
    }
    Ok(result)
}

/// The canonical FastBuffer, loading the buffer module on first use.
pub(crate) fn fast_buffer(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if shared
        .borrow()
        .builtin_cache
        .get(FAST_BUFFER_CACHE)
        .is_none()
    {
        crate::host::shared_vm::commonjs::buffer_module(context)?;
    }
    let fast = shared
        .borrow()
        .builtin_cache
        .get(FAST_BUFFER_CACHE)
        .copied();
    fast.ok_or_else(|| RootedError::host("canonical FastBuffer missing"))
}

/// Internal users and public Buffer allocations share one Uint8Array subclass.
pub(crate) fn internal_module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let fast = fast_buffer(context)?;
    let module = context.object_rooted()?;
    let key = context.string_rooted("FastBuffer");
    context.define_data_property_rooted(module, key, fast, true, true, true)?;
    Ok(module)
}
