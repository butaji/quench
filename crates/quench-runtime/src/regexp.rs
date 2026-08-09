//! RegExp execution using the `regress` crate.

use std::rc::Rc;

use regress::{Flags, Regex};

use crate::{execute::VmError, ops::Builtin, value::Value};

/// Build a regress `Regex` from pattern and JS-style flags.
pub fn compile(pattern: &str, flags: &str) -> Result<Regex, String> {
    let reg_flags: Flags = flags.into();
    Regex::with_flags(pattern, reg_flags).map_err(|e| e.to_string())
}

/// Dispatch builtin to implementation.
pub fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::RegExpTest => Some(test(receiver, arguments)),
        Builtin::RegExpExec => Some(exec(receiver, arguments)),
        _ => None,
    }
}

fn build_re_flags(flags: &str) -> String {
    let mut f = String::new();
    if flags.contains('i') {
        f.push('i');
    }
    if flags.contains('m') {
        f.push('m');
    }
    f
}

/// Implement `RegExp.prototype.test(str)`.
pub fn test(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Object(_)) = receiver else {
        return Err(VmError::EvalError(
            "RegExp.prototype.test requires RegExp".to_string(),
        ));
    };
    let s = argument_string(arguments);
    let (source, flags, last_index) = extract_regex_parts(receiver.unwrap());
    let (search_start, search_string) = prepare_search(&s, &flags, last_index);
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re_flags = build_re_flags(&flags);
    let re = compile(pattern, &re_flags).map_err(VmError::EvalError)?;
    let found = re.find(search_string);
    let matched = found.is_some();
    if flags.contains('g') || flags.contains('y') {
        let new_index = if matched {
            found.unwrap().end() + search_start
        } else {
            0
        };
        set_last_index(receiver.unwrap(), new_index as f64);
    }
    Ok(Value::Boolean(matched))
}

/// Implement `RegExp.prototype.exec(str)`.
pub fn exec(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Object(_)) = receiver else {
        return Err(VmError::EvalError(
            "RegExp.prototype.exec requires RegExp".to_string(),
        ));
    };
    let s = argument_string(arguments);
    let (source, flags, last_index) = extract_regex_parts(receiver.unwrap());
    let (search_start, search_string) = prepare_search(&s, &flags, last_index);
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re_flags = build_re_flags(&flags);
    let re = compile(pattern, &re_flags).map_err(VmError::EvalError)?;
    if let Some(m) = re.find(search_string) {
        build_match_result(receiver.unwrap(), &s, m, search_start, &flags)
    } else {
        if flags.contains('g') || flags.contains('y') {
            set_last_index(receiver.unwrap(), 0.0);
        }
        Ok(Value::Null)
    }
}

fn build_match_result(
    receiver: &Value,
    s: &str,
    m: regress::Match,
    search_start: usize,
    flags: &str,
) -> Result<Value, VmError> {
    let new_index = m.end() + search_start;
    if flags.contains('g') || flags.contains('y') {
        set_last_index(receiver, new_index as f64);
    }
    let full_match = &s[m.start() + search_start..new_index];
    let mut result = vec![
        ("0".to_string(), Value::String(full_match.to_string())),
        (
            "index".to_string(),
            Value::Number((m.start() + search_start) as f64),
        ),
        ("input".to_string(), Value::String(s.to_string())),
    ];
    for (i, group) in m.groups().enumerate() {
        let val = match group {
            Some(range) => {
                let start = range.start + search_start;
                let end = range.end + search_start;
                Value::String(s[start..end].to_string())
            }
            None => Value::Undefined,
        };
        result.push((i.to_string(), val));
    }
    Ok(Value::Object(Rc::new(result)))
}

fn argument_string(arguments: &[Value]) -> String {
    arguments
        .first()
        .map_or_else(|| "undefined".to_string(), value_to_string)
}

fn extract_regex_parts(receiver: &Value) -> (String, String, usize) {
    let source = extract_source(receiver);
    let flags = extract_flags(receiver);
    let last_index = extract_last_index(receiver) as usize;
    (source, flags, last_index)
}

fn prepare_search<'a>(s: &'a str, flags: &str, last_index: usize) -> (usize, &'a str) {
    let search_start = if flags.contains('g') || flags.contains('y') {
        last_index.min(s.len())
    } else {
        0
    };
    (search_start, &s[search_start..])
}

fn extract_source(receiver: &Value) -> String {
    match receiver {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "source")
            .and_then(|(_, v)| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_flags(receiver: &Value) -> String {
    match receiver {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "flags")
            .and_then(|(_, v)| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_last_index(receiver: &Value) -> f64 {
    match receiver {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "lastIndex")
            .and_then(|(_, v)| {
                if let Value::Number(n) = v {
                    Some(*n)
                } else {
                    None
                }
            })
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

fn set_last_index(receiver: &Value, index: f64) {
    if let Value::Object(props) = receiver {
        let mut props = (**props).clone();
        if let Some((_, v)) = props.iter_mut().find(|(k, _)| k == "lastIndex") {
            *v = Value::Number(index);
        }
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}
