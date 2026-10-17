//! ANSI color facts and native util.styleText policy.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const DEFAULT_FOREGROUND: u16 = 39;
const BOLD: u16 = 1;
const DIM: u16 = 2;
pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
    inspect: RootId,
) -> Result<(), RootedError> {
    let null = context.null();
    let data = context.array_rooted(&[inspect, null])?;
    let style =
        context.host_function_with_data(crate::host::shared_vm::operation("styleText"), data)?;
    set(context, module, "styleText", style)
}

#[derive(Clone)]
struct Style {
    open: Vec<u16>,
    close: Vec<u16>,
    keep_close: bool,
}
impl Style {
    fn replace_inner(&self, text: &[u16]) -> Vec<u16> {
        let mut output = Vec::new();
        let mut cursor = 0;
        while cursor < text.len() {
            if !self.close.is_empty()
                && text[cursor..].starts_with(&self.close)
                && cursor + self.close.len() < text.len()
            {
                if self.keep_close {
                    output.extend_from_slice(&self.close);
                }
                output.extend_from_slice(&self.open);
                cursor += self.close.len();
            } else {
                output.push(text[cursor]);
                cursor += 1;
            }
        }
        output
    }
    fn apply(&self, text: &[u16]) -> Vec<u16> {
        let mut output = self.open.clone();
        output.extend(self.replace_inner(text));
        output.extend_from_slice(&self.close);
        output
    }
}
fn sequence(code: &str) -> Vec<u16> {
    format!("\x1b[{code}m").encode_utf16().collect()
}
fn codes(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<Style, RootedError> {
    let open = get(context, value, "0")?;
    let close = get(context, value, "1")?;
    let keep_close = context
        .rooted_value(open)
        .and_then(quench_runtime::Value::as_number)
        .is_some_and(|open| open == f64::from(BOLD) || open == f64::from(DIM));
    Ok(Style {
        open: sequence(&context.to_string(open)?),
        close: sequence(&context.to_string(close)?),
        keep_close,
    })
}
fn hex(format: &str) -> Option<Style> {
    let value = format.strip_prefix('#')?;
    if !matches!(value.len(), 3 | 6) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = if value.len() == 3 {
        value
            .chars()
            .flat_map(|character| [character, character])
            .collect::<String>()
    } else {
        value.to_owned()
    };
    let rgb = (0..3)
        .map(|channel| u8::from_str_radix(&expanded[channel * 2..channel * 2 + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(Style {
        open: sequence(&format!("38;2;{};{};{}", rgb[0], rgb[1], rgb[2])),
        close: sequence(&DEFAULT_FOREGROUND.to_string()),
        keep_close: false,
    })
}

pub(crate) fn style_text(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let format = args.first().copied().unwrap_or(undefined);
    let text = args.get(1).copied().unwrap_or(undefined);
    let options = args.get(2).copied().unwrap_or(undefined);
    let validate = if nullish(context, options) {
        undefined
    } else {
        get(context, options, "validateStream")?
    };
    let validate = if nullish(context, validate) {
        context.boolean(true)
    } else {
        validate
    };
    let data = context.host_function_data()?;
    let inspect = get(context, data, "0")?;
    let format_text = context.string_text(format)?;
    let units = context.string_code_units(text)?;
    if !context.truthy_rooted(validate)? {
        if let (Some(format_text), Some(units)) = (&format_text, &units) {
            let mut cache = get(context, data, "1")?;
            if nullish(context, cache) {
                cache = context.null_object_rooted()?;
                set(context, data, "1", cache)?;
                let colors = get(context, inspect, "colors")?;
                for key in context.own_keys_rooted(colors)? {
                    if context.string_text(key)?.is_none() {
                        continue;
                    }
                    let value = context.get_property_rooted(colors, key)?;
                    if !context.truthy_rooted(value)? {
                        continue;
                    }
                    let style = codes(context, value)?;
                    let open = context.string_units_rooted(&style.open);
                    let close = context.string_units_rooted(&style.close);
                    let keep = context.boolean(style.keep_close);
                    let snapshot = context.array_rooted(&[open, close, keep])?;
                    context.set_property_rooted(cache, key, snapshot, cache)?;
                }
            }
            if format_text == "none" {
                return Ok(text);
            }
            let cached = context.get_property_rooted(cache, format)?;
            let style = if !nullish(context, cached) {
                let open = get(context, cached, "0")?;
                let close = get(context, cached, "1")?;
                let keep = get(context, cached, "2")?;
                Some(Style {
                    open: context.string_code_units(open)?.unwrap_or_default(),
                    close: context.string_code_units(close)?.unwrap_or_default(),
                    keep_close: context.truthy_rooted(keep)?,
                })
            } else {
                hex(format_text)
            };
            if let Some(style) = style {
                return Ok(context.string_units_rooted(&style.apply(units)));
            }
        }
    }
    let units = match units {
        Some(units) => units,
        None => {
            return Err(super::util_shared_vm::argument_type_error(
                context, text, "text", "string",
            )?);
        }
    };
    if !context
        .rooted_value(options)
        .is_some_and(|value| value.is_undefined())
        && (!context.is_object_rooted(options)?
            || context.is_callable_rooted(options)?
            || context.is_array_rooted(options)?)
    {
        return Err(super::util_shared_vm::argument_type_error(
            context, options, "options", "object",
        )?);
    }
    let validate = match context
        .rooted_value(validate)
        .and_then(quench_runtime::Value::as_bool)
    {
        Some(validate) => validate,
        None => {
            return Err(super::util_shared_vm::argument_type_error(
                context,
                validate,
                "options.validateStream",
                "boolean",
            )?);
        }
    };
    let skip = if validate {
        let stream = if nullish(context, options) {
            undefined
        } else {
            get(context, options, "stream")?
        };
        let stream = if nullish(context, stream) {
            let global = context.global_root()?;
            let process = get(context, global, "process")?;
            get(context, process, "stdout")?
        } else {
            stream
        };
        if !valid_stream(context, stream)? {
            return Err(super::util_shared_vm::argument_type_error(
                context,
                stream,
                "stream",
                "ReadableStream, WritableStream, or Stream",
            )?);
        }
        !should_colorize(context, stream)?
    } else {
        false
    };
    let formats = if context.is_array_rooted(format)? {
        format
    } else {
        context.array_rooted(&[format])?
    };
    let colors = get(context, inspect, "colors")?;
    let mut processed = units;
    let mut opening = Vec::new();
    let mut closing = Vec::new();
    let iterator = context.iterator_rooted(formats)?;
    while let Some(format) = context.iterator_step_rooted(&iterator)? {
        let step = (|| -> Result<(), RootedError> {
            let name = context.string_text(format)?;
            if name.as_deref() == Some("none") {
                return Ok(());
            }
            let style = if let Some(name) = name.as_deref().filter(|name| name.starts_with('#')) {
                let Some(style) = hex(name) else {
                    return invalid_format(context, inspect, format, true).map(|_| ());
                };
                if skip {
                    return Ok(());
                }
                style
            } else {
                let value = context.get_property_rooted(colors, format)?;
                if !context.truthy_rooted(value)? {
                    return invalid_format(context, inspect, format, false).map(|_| ());
                }
                codes(context, value)?
            };
            opening.extend_from_slice(&style.open);
            closing.splice(0..0, style.close.iter().copied());
            processed = style.replace_inner(&processed);
            Ok(())
        })();
        if let Err(error) = step {
            let _ = context.iterator_close_rooted(&iterator);
            return Err(error);
        }
        context.release_root(format);
    }
    if skip {
        return Ok(text);
    }
    opening.extend(processed);
    opening.extend(closing);
    Ok(context.string_units_rooted(&opening))
}

fn stream_method(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
    name: &str,
) -> Result<bool, RootedError> {
    let value = get(context, stream, name)?;
    context.is_callable_rooted(value)
}
fn node_stream(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
) -> Result<bool, RootedError> {
    if !context.truthy_rooted(stream)? {
        return Ok(false);
    }
    for name in ["_readableState", "_writableState"] {
        let value = get(context, stream, name)?;
        if context.truthy_rooted(value)? {
            return Ok(true);
        }
    }
    Ok(
        (stream_method(context, stream, "write")? && stream_method(context, stream, "on")?)
            || (stream_method(context, stream, "pipe")? && stream_method(context, stream, "on")?),
    )
}
fn valid_stream(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
) -> Result<bool, RootedError> {
    if !context.truthy_rooted(stream)? {
        return Ok(false);
    }
    if !node_stream(context, stream)?
        && stream_method(context, stream, "pipeThrough")?
        && stream_method(context, stream, "getReader")?
        && stream_method(context, stream, "cancel")?
    {
        return Ok(true);
    }
    if !node_stream(context, stream)?
        && stream_method(context, stream, "getWriter")?
        && stream_method(context, stream, "abort")?
    {
        return Ok(true);
    }
    node_stream(context, stream)
}

fn should_colorize(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
) -> Result<bool, RootedError> {
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let env = get(context, process, "env")?;
    let forced = get(context, env, "FORCE_COLOR")?;
    if !context
        .rooted_value(forced)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(matches!(
            context.string_text(forced)?.as_deref(),
            Some("" | "1" | "true" | "2" | "3")
        ));
    }
    let tty = get(context, stream, "isTTY")?;
    if !context.truthy_rooted(tty)? {
        return Ok(false);
    }
    let depth = get(context, stream, "getColorDepth")?;
    if !context.is_callable_rooted(depth)? {
        return Ok(true);
    }
    let depth = context.call_rooted(depth, stream, &[])?;
    const MONOCHROME_DEPTH: f64 = 2.0;
    let monochrome = context.number(MONOCHROME_DEPTH);
    context.greater_than_rooted(depth, monochrome)
}
fn invalid_format(
    context: &mut NativeContext<'_, NodeHost>,
    inspect: RootId,
    format: RootId,
    hex: bool,
) -> Result<RootId, RootedError> {
    let requirement = if hex {
        "must be a valid hex color (#RGB or #RRGGBB)".to_owned()
    } else {
        let colors = get(context, inspect, "colors")?;
        let mut names = Vec::new();
        for key in context.own_keys_rooted(colors)? {
            if let Some(name) = context.string_text(key)? {
                names.push(format!("'{name}'"));
            }
        }
        format!("must be one of: {}", names.join(", "))
    };
    let received = if context.string_code_units(format)?.is_some() {
        let undefined = context.undefined();
        let value = super::util_shared_vm::inspect_quote(context, undefined, &[format])?;
        context.to_string(value)?
    } else {
        let undefined = context.undefined();
        let value = context.call_rooted(inspect, undefined, &[format])?;
        context.to_string(value)?
    };
    let error = context.type_error_rooted(&format!(
        "The argument 'format' {requirement}. Received {received}"
    ))?;
    let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
    set(context, error, "code", code)?;
    Err(context.throw(error))
}
fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}
