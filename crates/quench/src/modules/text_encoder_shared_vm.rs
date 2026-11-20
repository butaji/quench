//! WHATWG TextEncoder over the runtime's UTF-16 strings and byte views.

use crate::host::NodeHost;
use quench_runtime::{
    BuiltinBrand, DescriptorFields, NativeContext, RootId, RootedError, ValueType,
};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The constructor and its methods carry the same unforgeable instance key.
pub(crate) fn constructor(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0encoding:TextEncoder", |c| {
        c.with_startup_intrinsics(|c| {
            let operation = crate::host::shared_vm::operation;
            let brand = c.private_symbol_rooted(Some("TextEncoder"));
            let constructor = c.host_constructor_named_with_data(
                operation("textEncoderConstruct"),
                "TextEncoder",
                brand,
            )?;
            let prototype = c.get_named_property_rooted(constructor, "prototype")?;
            c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
            install_members(c, prototype, brand)?;
            install_symbols(c, prototype, brand)?;
            Ok(constructor)
        })
    })
}

/// The `encoding` getter and the encode methods.
fn install_members(c: &mut Context<'_>, prototype: RootId, brand: RootId) -> Result<(), RootedError> {
    let operation = crate::host::shared_vm::operation;
    let getter = c.host_function_named_with_data(operation("textEncoderEncoding"), "get encoding", brand)?;
    let key = c.string_rooted("encoding");
    c.define_property_fields_rooted(
        prototype,
        key,
        DescriptorFields { get: Some(getter), enumerable: Some(true), configurable: Some(true), ..Default::default() },
    )?;
    for (name, callback) in [("encode", "textEncoderEncode"), ("encodeInto", "textEncoderEncodeInto")] {
        let function = c.host_function_named_with_data(operation(callback), name, brand)?;
        c.define_named_data_property_rooted(prototype, name, function, true, true, true)?;
    }
    Ok(())
}

/// The custom inspection and `Symbol.toStringTag`.
fn install_symbols(c: &mut Context<'_>, prototype: RootId, brand: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = c.get_named_property_rooted(global, "Symbol")?;
    let tag = c.get_named_property_rooted(symbol, "toStringTag")?;
    let tag_name = c.string_rooted("TextEncoder");
    let symbol_for = c.get_named_property_rooted(symbol, "for")?;
    let name = c.string_rooted(super::keys::INSPECT_CUSTOM);
    let inspect_key = c.call_rooted(symbol_for, symbol, &[name])?;
    let inspect = c.host_function_named_with_data(
        crate::host::shared_vm::operation("textEncoderInspect"),
        "[nodejs.util.inspect.custom]",
        brand,
    )?;
    c.define_data_property_rooted(prototype, inspect_key, inspect, true, false, true)?;
    c.define_data_property_rooted(prototype, tag, tag_name, false, false, true)?;
    Ok(())
}

pub(crate) fn initialize(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let constructor = constructor(c)?;
    let global = c.global_root()?;
    c.define_named_data_property_rooted(global, "TextEncoder", constructor, true, false, true)?;
    Ok(c.undefined())
}

pub(crate) fn construct(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error =
            c.type_error_rooted("Class constructor TextEncoder cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let brand = c.host_function_data()?;
    let yes = c.boolean(true);
    c.define_data_property_rooted(this, brand, yes, false, false, false)?;
    Ok(this)
}

/// Host-private symbols read own slots only and never forward through proxies.
fn validate(c: &mut Context<'_>, this: RootId, getter: bool) -> Result<(), RootedError> {
    if c.is_object_rooted(this)? {
        let brand = c.host_function_data()?;
        let marked = c.get_property_rooted(this, brand)?;
        if c.truthy_rooted(marked)? {
            return Ok(());
        }
    }
    let message = if c
        .rooted_value(this)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        let name = if c.rooted_value(this).is_some_and(|value| value.is_null()) {
            "null"
        } else {
            "undefined"
        };
        let property = if getter { "#encoding" } else { "TextEncoder" };
        format!("Cannot read properties of {name} (reading '{property}')")
    } else if getter {
        "Cannot read private member #encoding from an object whose class did not declare it"
            .to_owned()
    } else {
        "Receiver must be an instance of class TextEncoder".to_owned()
    };
    let error = c.type_error_rooted(&message)?;
    Err(c.throw(error))
}

pub(crate) fn encoding(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    validate(c, this, true)?;
    Ok(c.string_rooted("utf-8"))
}

pub(crate) fn encode(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    validate(c, this, false)?;
    let units = match args.first().copied() {
        Some(input)
            if !c
                .rooted_value(input)
                .is_some_and(|value| value.is_undefined()) =>
        {
            c.to_string_code_units(input)?
        }
        _ => Vec::new(),
    };
    let bytes = super::buffer_codec::encode_units(&units, "utf8");
    c.uint8_array_from_rooted(bytes)
}

pub(crate) fn encode_into(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let source = args.first().copied().unwrap_or(undefined);
    if c.value_type_rooted(source) != Some(ValueType::String) {
        let error = super::util_shared_vm::argument_type_error(c, source, "src", "string")?;
        if let Some(exception) = error.exception {
            super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_TYPE")?;
        }
        return Err(error);
    }
    let destination = args.get(1).copied().unwrap_or(undefined);
    if !c.has_builtin_brand_rooted(destination, BuiltinBrand::Uint8Array)? {
        return Err(super::util_shared_vm::argument_type_error(
            c,
            destination,
            "dest",
            "Uint8Array",
        )?);
    }
    // Node validates both arguments before looking up the private method on this.
    validate(c, this, false)?;
    let units = c.to_string_code_units(source)?;
    let (read, written) = c
        .with_buffer_bytes_mut_rooted(destination, |bytes| {
            super::buffer_codec::encode_utf8_into(&units, bytes)
        })
        .unwrap_or_default();
    let read = c.number(read as f64);
    let written = c.number(written as f64);
    c.record_rooted(false, &["read", "written"], &[read, written])
}

pub(crate) fn inspect(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if args
        .first()
        .and_then(|depth| c.rooted_value(*depth))
        .and_then(|value| value.as_number())
        .is_some_and(|depth| depth < 0.0)
    {
        return Ok(this);
    }
    validate(c, this, true)?;
    let constructor = inspected_constructor(c, this)?;
    let prototype = c.record_rooted(false, &["constructor"], &[constructor])?;
    let encoding = c.string_rooted("utf-8");
    let object = c.record_rooted(false, &["encoding"], &[encoding])?;
    c.set_prototype_rooted(object, prototype)?;
    let inspect = super::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let options = args.get(1).copied().unwrap_or(undefined);
    c.call_rooted(inspect, undefined, &[object, options])
}

/// The first named constructor an own `constructor` data property along `object`'s
/// prototype chain holds, as Node's inspection names a class; TextEncoder past the chain.
fn inspected_constructor(c: &mut Context<'_>, object: RootId) -> Result<RootId, RootedError> {
    let mut prototype = object;
    while !c.rooted_value(prototype).is_some_and(|value| value.is_null()) {
        if let Some(constructor) = own_named_constructor(c, prototype)? {
            return Ok(constructor);
        }
        prototype = c.prototype_rooted(prototype)?;
    }
    constructor(c)
}

fn own_named_constructor(c: &mut Context<'_>, object: RootId) -> Result<Option<RootId>, RootedError> {
    let key = c.string_rooted("constructor");
    let Some(value) = c.own_property_fields_rooted(object, key)?.and_then(|fields| fields.value) else {
        return Ok(None);
    };
    if !c.is_callable_rooted(value)? {
        return Ok(None);
    }
    let name = c.get_named_property_rooted(value, "name")?;
    Ok((c.string_text(name)?.as_deref() != Some("")).then_some(value))
}
