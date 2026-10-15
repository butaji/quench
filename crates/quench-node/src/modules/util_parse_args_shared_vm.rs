//! Node argument parsing over the shared VM's original ECMAScript operations.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn define(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)
        .map(|_| ())
}
fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|v| v.is_null() || v.is_undefined())
}
fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|v| v.is_undefined())
}

/// Private captured primordials are VM values, so the ordinary GC owns their
/// lifetime and native calls retain the shared coercion/proxy/species semantics.
#[derive(Clone, Copy)]
struct Operations(RootId);
impl Operations {
    fn call(
        self,
        c: &mut Context<'_>,
        name: &str,
        receiver: RootId,
        args: &[RootId],
    ) -> Result<RootId, RootedError> {
        let function = get(c, self.0, name)?;
        c.call_rooted(function, receiver, args)
    }
    fn own(self, c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
        let receiver = c.undefined();
        let value = self.call(c, "objectHasOwn", receiver, &[object, key])?;
        c.truthy_rooted(value)
    }
    fn own_value(
        self,
        c: &mut Context<'_>,
        object: RootId,
        key: RootId,
    ) -> Result<RootId, RootedError> {
        if self.own(c, object, key)? {
            c.get_property_rooted(object, key)
        } else {
            Ok(c.undefined())
        }
    }
    fn named(self, c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
        let key = c.string_rooted(name);
        self.own_value(c, object, key)
    }
    fn option(
        self,
        c: &mut Context<'_>,
        options: RootId,
        name: RootId,
        property: &str,
    ) -> Result<RootId, RootedError> {
        if self.own(c, options, name)? {
            let config = c.get_property_rooted(options, name)?;
            self.named(c, config, property)
        } else {
            Ok(c.undefined())
        }
    }
    fn entries(self, c: &mut Context<'_>, object: RootId) -> Result<RootId, RootedError> {
        let receiver = c.undefined();
        self.call(c, "objectEntries", receiver, &[object])
    }
    fn push(self, c: &mut Context<'_>, array: RootId, value: RootId) -> Result<(), RootedError> {
        self.call(c, "arrayPush", array, &[value])?;
        Ok(())
    }
    fn char_at(
        self,
        c: &mut Context<'_>,
        value: RootId,
        index: usize,
    ) -> Result<RootId, RootedError> {
        let index = c.number(index as f64);
        self.call(c, "stringCharAt", value, &[index])
    }
    fn slice(
        self,
        c: &mut Context<'_>,
        value: RootId,
        start: usize,
        end: Option<RootId>,
    ) -> Result<RootId, RootedError> {
        let start = c.number(start as f64);
        let end = end.unwrap_or_else(|| c.undefined());
        self.call(c, "stringSlice", value, &[start, end])
    }
    fn starts(self, c: &mut Context<'_>, value: RootId, prefix: &str) -> Result<bool, RootedError> {
        let prefix = c.string_rooted(prefix);
        let answer = self.call(c, "stringStartsWith", value, &[prefix])?;
        c.truthy_rooted(answer)
    }
    fn string_type(
        self,
        c: &mut Context<'_>,
        options: RootId,
        name: RootId,
    ) -> Result<bool, RootedError> {
        let value = self.option(c, options, name, "type")?;
        let string = c.string_rooted("string");
        c.strict_equal_rooted(value, string)
    }
    fn find_long(
        self,
        c: &mut Context<'_>,
        options: RootId,
        short: RootId,
    ) -> Result<RootId, RootedError> {
        validate_object(c, options, &"options".encode_utf16().collect::<Vec<_>>())?;
        let entries = self.entries(c, options)?;
        let length = get(c, entries, "length")?;
        let length = c.to_number(length)? as usize;
        for index in 0..length {
            let entry = get(c, entries, &index.to_string())?;
            let config = get(c, entry, "1")?;
            let value = self.named(c, config, "short")?;
            if c.strict_equal_rooted(value, short)? {
                return get(c, entry, "0");
            }
        }
        Ok(short)
    }
}

fn original(c: &mut Context<'_>, holder: RootId, name: &str) -> Result<RootId, RootedError> {
    c.startup_data_property_rooted(holder, name)
        .ok_or_else(|| RootedError::host(format!("missing startup intrinsic {name}")))
}

pub(crate) fn function(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let api = {
        let api = c.null_object_rooted()?;
        let global = c.global_root()?;
        for (constructor, prototype, methods) in [
            (
                "Array",
                true,
                &[
                    ("slice", "arraySlice"),
                    ("shift", "arrayShift"),
                    ("unshift", "arrayUnshift"),
                    ("push", "arrayPush"),
                    ("map", "arrayMap"),
                ][..],
            ),
            (
                "Object",
                false,
                &[("hasOwn", "objectHasOwn"), ("entries", "objectEntries")][..],
            ),
            ("Reflect", false, &[("apply", "reflectApply")][..]),
            (
                "String",
                true,
                &[
                    ("charAt", "stringCharAt"),
                    ("startsWith", "stringStartsWith"),
                    ("includes", "stringIncludes"),
                    ("indexOf", "stringIndexOf"),
                    ("slice", "stringSlice"),
                ][..],
            ),
        ] {
            let constructor = original(c, global, constructor)?;
            let owner = if prototype {
                original(c, constructor, "prototype")?
            } else {
                constructor
            };
            for &(method, key) in methods {
                let function = original(c, owner, method)?;
                define(c, api, key, function)?;
            }
        }
        api
    };
    c.host_function_with_data(crate::host::shared_vm::operation("parseArgs"), api)
}

fn positional(c: &mut Context<'_>, index: RootId, value: RootId) -> Result<RootId, RootedError> {
    let token = c.object_rooted()?;
    let kind = c.string_rooted("positional");
    define(c, token, "kind", kind)?;
    define(c, token, "index", index)?;
    define(c, token, "value", value)?;
    Ok(token)
}
fn option_token(
    c: &mut Context<'_>,
    name: RootId,
    raw: RootId,
    index: RootId,
    value: RootId,
    inline: Option<bool>,
) -> Result<RootId, RootedError> {
    let token = c.object_rooted()?;
    let kind = c.string_rooted("option");
    define(c, token, "kind", kind)?;
    define(c, token, "name", name)?;
    define(c, token, "rawName", raw)?;
    define(c, token, "index", index)?;
    define(c, token, "value", value)?;
    let inline = match inline {
        Some(value) => c.boolean(value),
        None => c.undefined(),
    };
    define(c, token, "inlineValue", inline)?;
    Ok(token)
}

fn type_error(
    c: &mut Context<'_>,
    value: RootId,
    name: &[u16],
    expected: &str,
) -> Result<RootedError, RootedError> {
    let error = super::util_shared_vm::argument_type_error_units(c, value, name, expected)?;
    super::net_options_shared_vm::validator_error(c, error, "ERR_INVALID_ARG_TYPE")
}
fn validate_object(c: &mut Context<'_>, value: RootId, name: &[u16]) -> Result<(), RootedError> {
    if nullish(c, value) || !c.is_object_rooted(value)? || c.is_array_rooted(value)? {
        return Err(type_error(c, value, name, "object")?);
    }
    Ok(())
}
fn validate_boolean(c: &mut Context<'_>, value: RootId, name: &[u16]) -> Result<bool, RootedError> {
    match c.rooted_value(value).and_then(|v| v.as_bool()) {
        Some(value) => Ok(value),
        None => Err(type_error(c, value, name, "boolean")?),
    }
}
fn validate_array(c: &mut Context<'_>, value: RootId, name: &[u16]) -> Result<(), RootedError> {
    if !c.is_array_rooted(value)? {
        return Err(type_error(c, value, name, "Array")?);
    }
    Ok(())
}

fn text_is(c: &mut Context<'_>, value: RootId, text: &str) -> Result<bool, RootedError> {
    let expected = c.string_rooted(text);
    c.strict_equal_rooted(value, expected)
}
fn affix(
    c: &mut Context<'_>,
    prefix: &str,
    value: RootId,
    suffix: &str,
) -> Result<RootId, RootedError> {
    let mut units = prefix.encode_utf16().collect::<Vec<_>>();
    units.extend(c.to_string_code_units(value)?);
    units.extend(suffix.encode_utf16());
    Ok(c.string_units_rooted(&units))
}
fn length_above(c: &mut Context<'_>, value: RootId, minimum: f64) -> Result<bool, RootedError> {
    let length = get(c, value, "length")?;
    let minimum = c.number(minimum);
    c.greater_than_rooted(length, minimum)
}
fn length_at_most(c: &mut Context<'_>, value: RootId, maximum: f64) -> Result<bool, RootedError> {
    let length = get(c, value, "length")?;
    let maximum = c.number(maximum);
    // The constant cannot run guest conversion hooks; reversing the operands
    // preserves both numeric comparison and the observable conversion order.
    c.greater_equal_rooted(maximum, length)
}
fn lone_short(c: &mut Context<'_>, op: Operations, arg: RootId) -> Result<bool, RootedError> {
    let length = get(c, arg, "length")?;
    let two = c.number(2.0);
    if !c.strict_equal_rooted(length, two)? {
        return Ok(false);
    }
    let first = op.char_at(c, arg, 0)?;
    if !text_is(c, first, "-")? {
        return Ok(false);
    }
    let second = op.char_at(c, arg, 1)?;
    Ok(!text_is(c, second, "-")?)
}
fn short_form(c: &mut Context<'_>, op: Operations, arg: RootId) -> Result<bool, RootedError> {
    if length_at_most(c, arg, 2.0)? {
        return Ok(false);
    }
    let first = op.char_at(c, arg, 0)?;
    if !text_is(c, first, "-")? {
        return Ok(false);
    }
    let second = op.char_at(c, arg, 1)?;
    Ok(!text_is(c, second, "-")?)
}
fn short_group(
    c: &mut Context<'_>,
    op: Operations,
    arg: RootId,
    options: RootId,
) -> Result<bool, RootedError> {
    if !short_form(c, op, arg)? {
        return Ok(false);
    }
    let short = op.char_at(c, arg, 1)?;
    let long = op.find_long(c, options, short)?;
    Ok(!op.string_type(c, options, long)?)
}
fn short_value(
    c: &mut Context<'_>,
    op: Operations,
    arg: RootId,
    options: RootId,
) -> Result<bool, RootedError> {
    validate_object(c, options, &"options".encode_utf16().collect::<Vec<_>>())?;
    if !short_form(c, op, arg)? {
        return Ok(false);
    }
    let short = op.char_at(c, arg, 1)?;
    let long = op.find_long(c, options, short)?;
    op.string_type(c, options, long)
}
fn long_form(
    c: &mut Context<'_>,
    op: Operations,
    arg: RootId,
    inline: bool,
) -> Result<bool, RootedError> {
    if !length_above(c, arg, 2.0)? || !op.starts(c, arg, "--")? {
        return Ok(false);
    }
    let equal = c.string_rooted("=");
    let offset = c.number(3.0);
    let contains = op.call(c, "stringIncludes", arg, &[equal, offset])?;
    Ok(c.truthy_rooted(contains)? == inline)
}

/// Called only by the captured Array.prototype.map after the option terminator.
/// Its counter is private VM data, retaining normal species/definition effects.
pub(crate) fn positional_map(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let index = get(c, state, "index")?;
    let index = c.to_number(index)? + 1.0;
    let index = c.number(index);
    define(c, state, "index", index)?;
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    positional(c, index, value)
}

fn tokenize(
    c: &mut Context<'_>,
    op: Operations,
    args: RootId,
    options: RootId,
) -> Result<RootId, RootedError> {
    let tokens = c.array_rooted(&[])?;
    let remaining = op.call(c, "arraySlice", args, &[])?;
    let mut index = -1.0;
    let mut grouped = 0_usize;
    while length_above(c, remaining, 0.0)? {
        let arg = op.call(c, "arrayShift", remaining, &[])?;
        let next = get(c, remaining, "0")?;
        if grouped > 0 {
            grouped -= 1;
        } else {
            index += 1.0;
        }
        let position = c.number(index);
        if text_is(c, arg, "--")? {
            let token = c.object_rooted()?;
            let kind = c.string_rooted("option-terminator");
            define(c, token, "kind", kind)?;
            define(c, token, "index", position)?;
            op.push(c, tokens, token)?;
            let state = c.null_object_rooted()?;
            define(c, state, "index", position)?;
            let callback = c.host_function_with_data(
                crate::host::shared_vm::operation("utilParseArgsPositional"),
                state,
            )?;
            let positionals = op.call(c, "arrayMap", remaining, &[callback])?;
            let push = get(c, op.0, "arrayPush")?;
            let receiver = c.undefined();
            op.call(c, "reflectApply", receiver, &[push, tokens, positionals])?;
            break;
        }
        if lone_short(c, op, arg)? {
            let short = op.char_at(c, arg, 1)?;
            let long = op.find_long(c, options, short)?;
            let mut value = c.undefined();
            let mut inline = None;
            if op.string_type(c, options, long)? && !nullish(c, next) {
                value = op.call(c, "arrayShift", remaining, &[])?;
                inline = Some(false);
            }
            let token = option_token(c, long, arg, position, value, inline)?;
            op.push(c, tokens, token)?;
            if !nullish(c, value) {
                index += 1.0;
            }
            continue;
        }
        if short_group(c, op, arg, options)? {
            let expanded = c.array_rooted(&[])?;
            let mut offset = 1_usize;
            loop {
                let length = get(c, arg, "length")?;
                let current = c.number(offset as f64);
                if !c.greater_than_rooted(length, current)? {
                    break;
                }
                let short = op.char_at(c, arg, offset)?;
                let long = op.find_long(c, options, short)?;
                let string = op.string_type(c, options, long)?;
                let last = if string {
                    let length = get(c, arg, "length")?;
                    let one = c.number(1.0);
                    let last = c.subtract_rooted(length, one)?;
                    c.strict_equal_rooted(current, last)?
                } else {
                    false
                };
                if !string || last {
                    let flag = affix(c, "-", short, "")?;
                    op.push(c, expanded, flag)?;
                } else {
                    let rest = op.slice(c, arg, offset, None)?;
                    let flag = affix(c, "-", rest, "")?;
                    op.push(c, expanded, flag)?;
                    break;
                }
                offset += 1;
            }
            let unshift = get(c, op.0, "arrayUnshift")?;
            let receiver = c.undefined();
            op.call(c, "reflectApply", receiver, &[unshift, remaining, expanded])?;
            let length = get(c, expanded, "length")?;
            grouped = c.to_number(length)? as usize;
            continue;
        }
        if short_value(c, op, arg, options)? {
            let short = op.char_at(c, arg, 1)?;
            let long = op.find_long(c, options, short)?;
            let value = op.slice(c, arg, 2, None)?;
            let raw = affix(c, "-", short, "")?;
            let token = option_token(c, long, raw, position, value, Some(true))?;
            op.push(c, tokens, token)?;
            continue;
        }
        if long_form(c, op, arg, false)? {
            let long = op.slice(c, arg, 2, None)?;
            let mut value = c.undefined();
            let mut inline = None;
            if op.string_type(c, options, long)? && !nullish(c, next) {
                value = op.call(c, "arrayShift", remaining, &[])?;
                inline = Some(false);
            }
            let token = option_token(c, long, arg, position, value, inline)?;
            op.push(c, tokens, token)?;
            if !nullish(c, value) {
                index += 1.0;
            }
            continue;
        }
        if long_form(c, op, arg, true)? {
            let equal = c.string_rooted("=");
            let equal_index = op.call(c, "stringIndexOf", arg, &[equal])?;
            let long = op.slice(c, arg, 2, Some(equal_index))?;
            let start = c.to_number(equal_index)? + 1.0;
            let value = op.slice(c, arg, start as usize, None)?;
            let raw = affix(c, "--", long, "")?;
            let token = option_token(c, long, raw, position, value, Some(true))?;
            op.push(c, tokens, token)?;
            continue;
        }
        let token = positional(c, position, arg)?;
        op.push(c, tokens, token)?;
    }
    Ok(tokens)
}

fn validate_element(
    c: &mut Context<'_>,
    value: RootId,
    name: &[u16],
    string: bool,
) -> Result<(), RootedError> {
    if string {
        if c.string_code_units(value)?.is_none() {
            return Err(type_error(c, value, name, "string")?);
        }
    } else {
        validate_boolean(c, value, name)?;
    }
    Ok(())
}
fn validation_field(name: &[u16], suffix: &str) -> Vec<u16> {
    name.iter().copied().chain(suffix.encode_utf16()).collect()
}

fn validate_options(
    c: &mut Context<'_>,
    op: Operations,
    options: RootId,
) -> Result<(), RootedError> {
    validate_object(c, options, &"options".encode_utf16().collect::<Vec<_>>())?;
    let entries = op.entries(c, options)?;
    let length = get(c, entries, "length")?;
    let length = c.to_number(length)? as usize;
    for index in 0..length {
        let entry = get(c, entries, &index.to_string())?;
        let key = get(c, entry, "0")?;
        let config = get(c, entry, "1")?;
        let mut label = "options.".encode_utf16().collect::<Vec<_>>();
        label.extend(c.to_string_code_units(key)?);
        validate_object(c, config, &label)?;
        let kind = op.named(c, config, "type")?;
        let string = text_is(c, kind, "string")?;
        if !string && !text_is(c, kind, "boolean")? {
            let error = super::util_shared_vm::argument_type_error_with_expected_units(
                c,
                kind,
                &validation_field(&label, ".type"),
                "('string|boolean')",
            )?;
            return Err(super::net_options_shared_vm::validator_error(
                c,
                error,
                "ERR_INVALID_ARG_TYPE",
            )?);
        }
        let short_key = c.string_rooted("short");
        if op.own(c, config, short_key)? {
            let short = get(c, config, "short")?;
            let Some(units) = c.string_code_units(short)? else {
                return Err(type_error(
                    c,
                    short,
                    &validation_field(&label, ".short"),
                    "string",
                )?);
            };
            if units.len() != 1 {
                return Err(super::util_shared_vm::argument_value_error_units(
                    c,
                    short,
                    &validation_field(&label, ".short"),
                    "must be a single character",
                )?);
            }
        }
        let multiple = op.named(c, config, "multiple")?;
        let key = c.string_rooted("multiple");
        if op.own(c, config, key)? {
            validate_boolean(c, multiple, &validation_field(&label, ".multiple"))?;
        }
        let default = op.named(c, config, "default")?;
        if !undefined(c, default) {
            let label = validation_field(&label, ".default");
            if c.truthy_rooted(multiple)? {
                validate_array(c, default, &label)?;
                let mut index = 0_usize;
                loop {
                    let length = get(c, default, "length")?;
                    let position = c.number(index as f64);
                    if !c.greater_than_rooted(length, position)? {
                        break;
                    }
                    let value = get(c, default, &index.to_string())?;
                    validate_element(
                        c,
                        value,
                        &validation_field(&label, &format!("[{index}]")),
                        string,
                    )?;
                    index += 1;
                }
            } else {
                validate_element(c, default, &label, string)?;
            }
        }
    }
    Ok(())
}

struct Message(Vec<u16>);
impl Message {
    fn new(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }
    fn text(&mut self, text: &str) {
        self.0.extend(text.encode_utf16());
    }
    fn value(&mut self, c: &mut Context<'_>, value: RootId) -> Result<(), RootedError> {
        self.0.extend(c.to_string_code_units(value)?);
        Ok(())
    }
    fn property(
        &mut self,
        c: &mut Context<'_>,
        object: RootId,
        name: &str,
    ) -> Result<(), RootedError> {
        let value = get(c, object, name)?;
        self.value(c, value)
    }
    fn finish(self, c: &mut Context<'_>) -> RootId {
        c.string_units_rooted(&self.0)
    }
}
fn parse_error(
    c: &mut Context<'_>,
    code: &'static str,
    message: RootId,
) -> Result<RootedError, RootedError> {
    let error = c.type_error_rooted("")?;
    let key = c.string_rooted("message");
    c.set_property_rooted(error, key, message, error)?;
    super::node_errors_shared_vm::decorate_type_error(c, error, code)?;
    Ok(c.throw(error))
}
fn unknown_option(
    c: &mut Context<'_>,
    raw: RootId,
    allow_positionals: bool,
) -> Result<RootedError, RootedError> {
    // Node builds the suggestion (including toJSON effects) before interpolating
    // the raw option into the main message.
    let json = if allow_positionals {
        Some(c.stringify_json_rooted(raw)?)
    } else {
        None
    };
    let mut message = Message::new("Unknown option '");
    message.value(c, raw)?;
    message.text("'");
    if let Some(json) = json {
        message.text(". To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- ");
        message.value(c, json)?;
    }
    let message = message.finish(c);
    parse_error(c, "ERR_PARSE_ARGS_UNKNOWN_OPTION", message)
}
fn check_usage(
    c: &mut Context<'_>,
    op: Operations,
    options: RootId,
    token: RootId,
    allow_negative: bool,
    allow_positionals: bool,
) -> Result<(), RootedError> {
    let mut name = get(c, token, "name")?;
    if !op.own(c, options, name)? {
        let negative = allow_negative && op.starts(c, name, "no-")?;
        if negative {
            name = op.slice(c, name, 3, None)?;
        }
        let known = negative && op.own(c, options, name)?;
        let boolean = if known {
            let kind = op.option(c, options, name, "type")?;
            text_is(c, kind, "boolean")?
        } else {
            false
        };
        if !boolean {
            let raw = get(c, token, "rawName")?;
            return Err(unknown_option(c, raw, allow_positionals)?);
        }
    }
    let short = op.option(c, options, name, "short")?;
    let mut label = Message::new("");
    if c.truthy_rooted(short)? {
        label.text("-");
        label.value(c, short)?;
        label.text(", ");
    }
    label.text("--");
    label.value(c, name)?;
    let label = label.finish(c);
    let kind = op.option(c, options, name, "type")?;
    if text_is(c, kind, "string")? {
        let value = get(c, token, "value")?;
        if c.string_code_units(value)?.is_none() {
            let message = affix(c, "Option '", label, " <value>' argument missing")?;
            return Err(parse_error(
                c,
                "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
                message,
            )?);
        }
    }
    if text_is(c, kind, "boolean")? {
        let value = get(c, token, "value")?;
        if !nullish(c, value) {
            let message = affix(c, "Option '", label, "' does not take an argument")?;
            return Err(parse_error(
                c,
                "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
                message,
            )?);
        }
    }
    Ok(())
}
fn check_ambiguous(c: &mut Context<'_>, op: Operations, token: RootId) -> Result<(), RootedError> {
    let inline = get(c, token, "inlineValue")?;
    if c.truthy_rooted(inline)? {
        return Ok(());
    }
    let value = get(c, token, "value")?;
    if nullish(c, value) || !length_above(c, value, 1.0)? {
        return Ok(());
    }
    let first = op.char_at(c, value, 0)?;
    if !text_is(c, first, "-")? {
        return Ok(());
    }
    let raw = get(c, token, "rawName")?;
    let mut example = Message::new("'");
    if op.starts(c, raw, "--")? {
        example.property(c, token, "rawName")?;
        example.text("=-XYZ'");
    } else {
        example.text("--");
        example.property(c, token, "name")?;
        example.text("=-XYZ' or '");
        example.property(c, token, "rawName")?;
        example.text("-XYZ'");
    }
    let example = example.finish(c);
    let mut message = Message::new("Option '");
    message.property(c, token, "rawName")?;
    message.text("' argument is ambiguous.\nDid you forget to specify the option argument for '");
    message.property(c, token, "rawName")?;
    message.text("'?\nTo specify an option argument starting with a dash use ");
    message.value(c, example)?;
    message.text(".");
    let message = message.finish(c);
    Err(parse_error(
        c,
        "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
        message,
    )?)
}

fn store_option(
    c: &mut Context<'_>,
    op: Operations,
    token: RootId,
    options: RootId,
    values: RootId,
    allow_negative: bool,
) -> Result<(), RootedError> {
    let mut name = get(c, token, "name")?;
    let mut value = get(c, token, "value")?;
    if text_is(c, name, "__proto__")? {
        return Ok(());
    }
    if allow_negative && op.starts(c, name, "no-")? && undefined(c, value) {
        name = op.slice(c, name, 3, None)?;
        c.assign_named_property_rooted(token, "name", name)?;
        value = c.boolean(false);
    }
    let value = if nullish(c, value) {
        c.boolean(true)
    } else {
        value
    };
    let multiple = op.option(c, options, name, "multiple")?;
    if c.truthy_rooted(multiple)? {
        let existing = c.get_property_rooted(values, name)?;
        if c.truthy_rooted(existing)? {
            let existing = c.get_property_rooted(values, name)?;
            op.push(c, existing, value)?;
        } else {
            let array = c.array_rooted(&[value])?;
            c.set_property_rooted(values, name, array, values)?;
        }
    } else {
        c.set_property_rooted(values, name, value, values)?;
    }
    Ok(())
}
fn config_value(
    c: &mut Context<'_>,
    op: Operations,
    config: RootId,
    name: &str,
    fallback: RootId,
) -> Result<RootId, RootedError> {
    let value = op.named(c, config, name)?;
    Ok(if nullish(c, value) { fallback } else { value })
}
fn main_args(c: &mut Context<'_>, op: Operations) -> Result<RootId, RootedError> {
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let argv = get(c, process, "argv")?;
    // Read the normalized startup fact, independent of public argv metadata.
    let eval = c.host_mut().eval_source_nonempty;
    let start = c.number(if eval { 1.0 } else { 2.0 });
    op.call(c, "arraySlice", argv, &[start])
}

pub(crate) fn parse(
    c: &mut Context<'_>,
    _: RootId,
    arguments: &[RootId],
) -> Result<RootId, RootedError> {
    let op = Operations(c.host_function_data()?);
    let mut config = arguments.first().copied().unwrap_or_else(|| c.undefined());
    if undefined(c, config) {
        config = c.null_object_rooted()?;
    }
    let args = op.named(c, config, "args")?;
    let args = if nullish(c, args) {
        main_args(c, op)?
    } else {
        args
    };
    let yes = c.boolean(true);
    let strict = config_value(c, op, config, "strict", yes)?;
    let strict_truthy = c.truthy_rooted(strict)?;
    let default_positionals = c.boolean(!strict_truthy);
    let allow_positionals = config_value(c, op, config, "allowPositionals", default_positionals)?;
    let no = c.boolean(false);
    let tokens_requested = config_value(c, op, config, "tokens", no)?;
    let allow_negative = config_value(c, op, config, "allowNegative", no)?;
    let options = op.named(c, config, "options")?;
    let options = if nullish(c, options) {
        c.null_object_rooted()?
    } else {
        options
    };
    validate_array(c, args, &"args".encode_utf16().collect::<Vec<_>>())?;
    let strict = validate_boolean(c, strict, &"strict".encode_utf16().collect::<Vec<_>>())?;
    let allow_positionals = validate_boolean(
        c,
        allow_positionals,
        &"allowPositionals".encode_utf16().collect::<Vec<_>>(),
    )?;
    let tokens_requested = validate_boolean(
        c,
        tokens_requested,
        &"tokens".encode_utf16().collect::<Vec<_>>(),
    )?;
    let allow_negative = validate_boolean(
        c,
        allow_negative,
        &"allowNegative".encode_utf16().collect::<Vec<_>>(),
    )?;
    validate_options(c, op, options)?;

    let tokens = tokenize(c, op, args, options)?;
    let values = c.null_object_rooted()?;
    let positionals = c.array_rooted(&[])?;
    let result = c.object_rooted()?;
    define(c, result, "values", values)?;
    define(c, result, "positionals", positionals)?;
    if tokens_requested {
        define(c, result, "tokens", tokens)?;
    }
    let length = get(c, tokens, "length")?;
    let length = c.to_number(length)? as usize;
    for index in 0..length {
        let key = c.string_rooted(&index.to_string());
        if !c.has_property_rooted(tokens, key)? {
            continue;
        }
        let token = c.get_property_rooted(tokens, key)?;
        let kind = get(c, token, "kind")?;
        if text_is(c, kind, "option")? {
            if strict {
                check_usage(c, op, options, token, allow_negative, allow_positionals)?;
                check_ambiguous(c, op, token)?;
            }
            store_option(c, op, token, options, values, allow_negative)?;
        } else {
            let kind = get(c, token, "kind")?;
            if text_is(c, kind, "positional")? {
                if !allow_positionals {
                    let mut message = Message::new("Unexpected argument '");
                    message.property(c, token, "value")?;
                    message.text("'. This command does not take positional arguments");
                    let message = message.finish(c);
                    return Err(parse_error(
                        c,
                        "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL",
                        message,
                    )?);
                }
                let value = get(c, token, "value")?;
                op.push(c, positionals, value)?;
            }
        }
    }
    let entries = op.entries(c, options)?;
    let length = get(c, entries, "length")?;
    let length = c.to_number(length)? as usize;
    for index in 0..length {
        let entry = get(c, entries, &index.to_string())?;
        let name = get(c, entry, "0")?;
        let config = get(c, entry, "1")?;
        let default = op.named(c, config, "default")?;
        if !undefined(c, default) {
            let current = c.get_property_rooted(values, name)?;
            if undefined(c, current) {
                let default = op.named(c, config, "default")?;
                if !text_is(c, name, "__proto__")? {
                    c.set_property_rooted(values, name, default, values)?;
                }
            }
        }
    }
    Ok(result)
}
