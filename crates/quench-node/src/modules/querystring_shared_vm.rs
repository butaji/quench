//! Query-string API built from the runtime-neutral scanner and codec.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

use crate::modules::querystring_data::{self, EQ_DEFAULT, MAX_KEYS_DEFAULT, SEP_DEFAULT};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let parse = context.host_function(crate::host::shared_vm::operation("querystringParse"))?;
    let stringify =
        context.host_function(crate::host::shared_vm::operation("querystringStringify"))?;
    set(context, module, "parse", parse)?;
    set(context, module, "decode", parse)?;
    set(context, module, "stringify", stringify)?;
    set(context, module, "encode", stringify)?;
    let escape = context.evaluate_script_rooted(
        "(value) => encodeURIComponent(String(value)).replace(/[!'()*]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)",
        "node:querystring/escape.js",
    )?;
    set(context, module, "escape", escape)?;
    let unescape = context.evaluate_script_rooted(
        "(value) => { const text = String(value).replace(/\\+/g, ' '); try { return decodeURIComponent(text); } catch { return text; } }",
        "node:querystring/unescape.js",
    )?;
    set(context, module, "unescape", unescape)?;
    Ok(module)
}

pub(crate) fn parse(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let object = context.null_object_rooted()?;
    let Some(input) = args.first().copied() else {
        return Ok(object);
    };
    let Some(input) = context.string_text(input)? else {
        return Ok(object);
    };
    let units = input.encode_utf16().collect::<Vec<_>>();
    let separator = parameter_units(context, args.get(1).copied(), SEP_DEFAULT)?;
    let equality = parameter_units(context, args.get(2).copied(), EQ_DEFAULT)?;
    let max_keys = max_keys(context, args.get(3).copied())?;
    let entries = querystring_data::parse_entries(
        &units,
        &separator,
        &equality,
        max_keys,
        false,
        |component| querystring_data::decode_default_component(component, false),
    );
    for (key, values) in entries {
        let value = if values.len() == 1 {
            context.string_rooted(&values[0])
        } else {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)?
        };
        set(context, object, &key, value)?;
    }
    Ok(object)
}

fn parameter_units(
    context: &mut NativeContext<'_, NodeHost>,
    argument: Option<RootId>,
    default: &[u16],
) -> Result<Vec<u16>, RootedError> {
    let Some(argument) = argument else {
        return Ok(default.to_vec());
    };
    if !context.truthy_rooted(argument)? {
        return Ok(default.to_vec());
    }
    Ok(context.to_string(argument)?.encode_utf16().collect())
}

fn max_keys(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<i64, RootedError> {
    let Some(options) = options else {
        return Ok(MAX_KEYS_DEFAULT);
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(MAX_KEYS_DEFAULT);
    }
    let key = context.string_rooted("maxKeys");
    let value = context.get_property_rooted(options, key)?;
    Ok(
        match context
            .rooted_value(value)
            .and_then(|value| value.as_number())
        {
            Some(limit) if limit > 0.0 => limit as i64,
            Some(_) => -1,
            None => MAX_KEYS_DEFAULT,
        },
    )
}

pub(crate) fn stringify(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(object) = args.first().copied() else {
        return Ok(context.string_rooted(""));
    };
    if !context.is_object_rooted(object)? {
        return Ok(context.string_rooted(""));
    }

    let separator = parameter_text(context, args.get(1).copied(), "&")?;
    let equality = parameter_text(context, args.get(2).copied(), "=")?;
    let keys = enumerable_keys(context, object)?;
    let mut output = String::new();
    for key in keys {
        let key_root = context.string_rooted(&key);
        let value = context.get_property_rooted(object, key_root)?;
        append_value(context, &mut output, &separator, &equality, &key, value)?;
    }
    Ok(context.string_rooted(&output))
}

fn append_value(
    context: &mut NativeContext<'_, NodeHost>,
    output: &mut String,
    separator: &str,
    equality: &str,
    key: &str,
    value: RootId,
) -> Result<(), RootedError> {
    if is_array(context, value)? {
        append_array(context, output, separator, equality, key, value)
    } else {
        let separator_before = !output.is_empty();
        append_field(
            context,
            output,
            separator_before,
            separator,
            equality,
            key,
            value,
        )
    }
}

fn append_array(
    context: &mut NativeContext<'_, NodeHost>,
    output: &mut String,
    separator: &str,
    equality: &str,
    key: &str,
    values: RootId,
) -> Result<(), RootedError> {
    let length = property(context, values, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    for index in 0..length {
        let value = property(context, values, &index.to_string())?;
        let separator_before = !output.is_empty() || index > 0;
        append_field(
            context,
            output,
            separator_before,
            separator,
            equality,
            key,
            value,
        )?;
    }
    Ok(())
}

fn parameter_text(
    context: &mut NativeContext<'_, NodeHost>,
    argument: Option<RootId>,
    default: &str,
) -> Result<String, RootedError> {
    let Some(argument) = argument else {
        return Ok(default.to_owned());
    };
    if !context.truthy_rooted(argument)? {
        return Ok(default.to_owned());
    }
    context.to_string(argument)
}

fn enumerable_keys(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
) -> Result<Vec<String>, RootedError> {
    let global = context.global_root()?;
    let object_constructor = property(context, global, "Object")?;
    let keys_function = property(context, object_constructor, "keys")?;
    let keys = context.call_rooted(keys_function, object_constructor, &[object])?;
    let length = property(context, keys, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    (0..length)
        .map(|index| {
            let key = property(context, keys, &index.to_string())?;
            context.to_string(key)
        })
        .collect()
}

fn is_array(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let array_constructor = property(context, global, "Array")?;
    let is_array_function = property(context, array_constructor, "isArray")?;
    let result = context.call_rooted(is_array_function, array_constructor, &[value])?;
    Ok(context
        .rooted_value(result)
        .and_then(|value| value.as_bool())
        .unwrap_or(false))
}

fn append_field(
    context: &mut NativeContext<'_, NodeHost>,
    output: &mut String,
    separator_before: bool,
    separator: &str,
    equality: &str,
    key: &str,
    value: RootId,
) -> Result<(), RootedError> {
    if separator_before {
        output.push_str(separator);
    }
    output.push_str(&querystring_data::encode_component(key));
    output.push_str(equality);
    output.push_str(&querystring_data::encode_component(&stringify_value(
        context, value,
    )?));
    Ok(())
}

fn stringify_value(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    if context.is_symbol_rooted(value)? {
        return Ok(String::new());
    }
    if context.string_text(value)?.is_some() {
        return context.to_string(value);
    }
    if let Some(rooted) = context.rooted_value(value) {
        if rooted.is_null()
            || rooted.is_undefined()
            || rooted.as_number().is_some_and(|number| !number.is_finite())
        {
            return Ok(String::new());
        }
        if let Some(boolean) = rooted.as_bool() {
            return Ok(boolean.to_string());
        }
        if rooted.as_number().is_some() {
            return context.to_string(value);
        }
    }
    if context.is_object_rooted(value)? || context.is_callable_rooted(value)? {
        return Ok(String::new());
    }
    context.to_string(value)
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
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
            "cannot install shared querystring property {name}"
        )))
    }
}
