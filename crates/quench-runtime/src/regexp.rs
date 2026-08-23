include!("regexp_validation.rs");
include!("regexp_named_groups.rs");
include!("regexp_surrogates.rs");

pub fn compile(pattern: &str, flags: &str) -> Result<Regex, String> {
    validate_literal_ranges(pattern)?;
    validate_flags(flags)?;
    let rewritten = split_surrogate_classes(pattern);
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| {
        Regex::with_flags(&rewritten, reg_flags)
    }))
    .map_err(|_| "invalid regular expression".to_string())?
    .map_err(|e| e.to_string())
}

pub fn validate_unicode(pattern: &str, flags: &str) -> Result<(), String> {
    validate_literal_ranges(pattern).map_err(|error| format!("SyntaxError: {error}"))?;
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| Regex::with_flags(pattern, reg_flags)))
        .map_err(|_| "SyntaxError: invalid regular expression".to_string())?
        .map(|_| ())
        .map_err(|error| format!("SyntaxError: {error}"))
}

fn validate_literal_ranges(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        index += 1;
        if index < bytes.len() && bytes[index] == b'^' {
            index += 1;
        }
        while index < bytes.len() && bytes[index] != b']' {
            let (left_value, next) = class_atom(bytes, index);
            index = next;
            let Some(left_value) = left_value else {
                continue;
            };
            if index < bytes.len()
                && bytes[index] == b'-'
                && index + 1 < bytes.len()
                && bytes[index + 1] != b']'
            {
                index += 1;
                let (right, next) = class_atom(bytes, index);
                index = next;
                if let Some(right) = right {
                    if right < left_value {
                        return Err("invalid character range".to_string());
                    }
                }
            }
        }
        if index < bytes.len() {
            index += 1;
        }
    }
    Ok(())
}

/// Decode one simple character-class atom. Unknown escapes are intentionally
/// returned as `None`: they are not safe to order, but are valid regex atoms.
fn class_atom(bytes: &[u8], mut index: usize) -> (Option<u32>, usize) {
    if index >= bytes.len() {
        return (None, index);
    }
    if bytes[index] != b'\\' {
        return (Some(bytes[index] as u32), index + 1);
    }
    index += 1;
    if index >= bytes.len() {
        return (None, index);
    }
    let kind = bytes[index];
    match kind {
        b'x' if index + 2 < bytes.len() => {
            let value = hex_value(bytes[index + 1])
                .and_then(|a| hex_value(bytes[index + 2]).map(|b| (a << 4) | b));
            (value, index + 3)
        }
        b'u' if index + 4 < bytes.len() => {
            let mut value = 0u32;
            for offset in 1..=4 {
                let Some(digit) = hex_value(bytes[index + offset]) else {
                    return (None, index + 1);
                };
                value = (value << 4) | digit;
            }
            (Some(value), index + 5)
        }
        _ => (Some(kind as u32), index + 1),
    }
}

fn hex_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'f' => Some((byte - b'a' + 10) as u32),
        b'A'..=b'F' => Some((byte - b'A' + 10) as u32),
        _ => None,
    }
}

pub fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::RegExpCompile => Some(compile_method_for_vm(receiver, arguments)),
        Builtin::RegExpEscape => Some(escape(arguments)),
        Builtin::RegExpTest => Some(test(receiver, arguments)),
        Builtin::RegExpExec => Some(exec(receiver, arguments)),
        Builtin::RegExpSymbolMatch => Some(symbol_match(receiver, arguments)),
        Builtin::RegExpSymbolSearch => Some(symbol_search(receiver, arguments)),
        Builtin::RegExpSymbolReplace => Some(symbol_replace(receiver, arguments)),
        Builtin::RegExpSymbolSplit => Some(symbol_split(receiver, arguments)),
        Builtin::RegExpSymbolMatchAll => Some(symbol_match_all(receiver, arguments)),
        Builtin::RegExpStringIteratorNext => Some(crate::collections::iterator::next(receiver)),
        Builtin::StringIteratorNext => Some(crate::collections::iterator::next_string(receiver)),
        _ => None,
    }
}

pub(crate) fn compile_method_for_vm(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    };
    if !has_regexp_internal_slot(receiver) {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    }
    let (pattern, flags) = compile_arguments(arguments)?;
    compile(&pattern, &flags).map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    update_compiled_receiver(receiver, &pattern, &flags)
}

fn compile_arguments(arguments: &[Value]) -> Result<(String, String), VmError> {
    let pattern_value = arguments.first().unwrap_or(&Value::Undefined);
    let pattern_is_regexp = has_regexp_internal_slot(pattern_value);
    let explicit_flags = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined));
    if pattern_is_regexp && explicit_flags.is_some() {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile flags with RegExp pattern",
        ));
    }
    let pattern = if pattern_is_regexp {
        internal_regexp_string(pattern_value, "source")?
    } else if matches!(pattern_value, Value::Undefined) {
        String::new()
    } else {
        crate::conversion::to_string(pattern_value)?
    };
    let flags = if pattern_is_regexp {
        internal_regexp_string(pattern_value, "flags")?
    } else {
        explicit_flags.map_or_else(|| Ok(String::new()), crate::conversion::to_string)?
    };
    Ok((pattern, flags))
}

fn internal_regexp_string(value: &Value, key: &str) -> Result<String, VmError> {
    let Value::Object(properties) = value else {
        return Err(crate::value::error::throw_type_error(
            "RegExp internal slot is unavailable",
        ));
    };
    let internal_key = match key {
        "source" => "\0regexp_source",
        "flags" => "\0regexp_flags",
        _ => key,
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| {
            (name == internal_key).then(|| match value {
                Value::BindingCell(cell) => match &*cell.borrow() {
                    Value::String(text) => Some(text.clone()),
                    _ => None,
                },
                Value::String(text) => Some(text.clone()),
                _ => None,
            })
        })
        .flatten()
        .ok_or_else(|| crate::value::error::throw_type_error("RegExp internal slot is unavailable"))
}

fn update_compiled_receiver(
    receiver: &Value,
    pattern: &str,
    flags: &str,
) -> Result<Value, VmError> {
    let Value::Object(properties) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    };
    for (key, value) in properties.iter() {
        let next = match key.as_str() {
            "source" => Some(Value::String(pattern.to_string())),
            "flags" => Some(Value::String(canonical_flags(flags))),
            "\0regexp_source" => Some(Value::String(pattern.to_string())),
            "\0regexp_flags" => Some(Value::String(flags.to_string())),
            _ => None,
        };
        if let (Some(next), Value::BindingCell(cell)) = (next, value) {
            cell.replace(next);
        }
    }
    crate::properties::assign_set_property(receiver, "lastIndex", Value::Number(0.0))?;
    Ok(receiver.clone())
}

include!("regexp_methods.rs");
include!("regexp_tail.rs");

#[cfg(test)]
mod tests {
    use super::has_regexp_internal_slot;
    use crate::value::{ObjectData, Value};

    #[test]
    fn regexp_slot_requires_intrinsic_marker() {
        let plain = Value::Object(ObjectData::new(Vec::new()).into());
        assert!(!has_regexp_internal_slot(&plain));
        let regexp = Value::Object(
            ObjectData::new(vec![("\0regexp".to_string(), Value::Boolean(true))]).into(),
        );
        assert!(has_regexp_internal_slot(&regexp));
    }
}
