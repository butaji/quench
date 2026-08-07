//! String.prototype methods that use RegExp

mod helpers;
pub use helpers::{
    apply_substitution, make_match_array, make_value_array, match_captures, replace_all_matches,
    split_by_regex,
};

use std::cell::RefCell;
use std::rc::Rc;

use regress::Regex;

use crate::value::convert::to_js_string;
use crate::value::{to_number, JsError, NativeFunction, Object, Value};
use crate::Context;

/// Register String.prototype methods that use RegExp
pub fn register_string_regex_methods(ctx: &mut Context) {
    if let Some(proto) = get_string_prototype(ctx) {
        let mut proto_mut = proto.borrow_mut();
        proto_mut.set(
            "match",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_match_impl))),
        );
        proto_mut.set(
            "search",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_search_impl))),
        );
        proto_mut.set(
            "replace",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_replace_impl))),
        );
        proto_mut.set(
            "split",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_split_impl))),
        );
        proto_mut.set(
            "replaceAll",
            Value::NativeFunction(Rc::new(NativeFunction::new(string_replace_all_impl))),
        );
    }
}

fn get_string_prototype(ctx: &mut Context) -> Option<Rc<RefCell<Object>>> {
    let string_proto = ctx.get_global("String")?;
    if let Value::Object(o) = string_proto {
        if let Some(Value::Object(po)) = o.borrow().get("prototype") {
            return Some(Rc::clone(&po));
        }
    }
    None
}

pub(crate) fn string_match_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.match requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first();

    if let Some(pattern) = pattern {
        return match_all_impl(&string, pattern);
    }
    Ok(Value::Null)
}

fn match_all_impl(string: &str, pattern: &Value) -> Result<Value, JsError> {
    if let Value::Object(ref obj) = pattern {
        if let Some(regex) = obj.borrow().internal_regex.clone() {
            let is_global = obj
                .borrow()
                .get("global")
                .map(|v| v == Value::Boolean(true))
                .unwrap_or(false);

            if !is_global {
                // Call exec via call_value_with_this so `this` is the RegExp object.
                let exec = obj.borrow().get("exec").unwrap_or(Value::Undefined);
                return crate::eval::call_value_with_this(
                    exec,
                    vec![Value::String(string.to_string())],
                    Value::Object(Rc::clone(obj)),
                );
            }
            return Ok(make_match_array(string, &regex));
        }
    }

    let pattern_str = to_js_string(pattern);
    if let Ok(regex) = Regex::new(&pattern_str) {
        return Ok(make_match_array(string, &regex));
    }
    Ok(Value::Null)
}

pub(crate) fn string_search_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.search requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first();

    if let Some(pattern) = pattern {
        let regex = match pattern {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let pattern_str = to_js_string(pattern);
                Regex::new(&pattern_str).ok()
            }
        };

        if let Some(regex) = regex {
            if let Some(m) = regex.find(&string) {
                return Ok(Value::Number(m.start() as f64));
            }
        }
    }

    Ok(Value::Number(-1.0))
}

pub(crate) fn string_replace_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.replace requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first().cloned();
    let replacement = args.get(1).cloned();

    if let (Some(pattern), Some(replacement)) = (pattern, replacement) {
        let regex = match &pattern {
            Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
            _ => {
                let pattern_str = to_js_string(&pattern);
                Regex::new(&pattern_str).ok()
            }
        };

        if let Some(regex) = regex {
            let is_global = match &pattern {
                Value::Object(ref obj) => obj
                    .borrow()
                    .get("global")
                    .map(|v| v == Value::Boolean(true))
                    .unwrap_or(false),
                _ => false,
            };
            if is_global {
                return Ok(Value::String(replace_all_with_value(
                    &string,
                    &regex,
                    &replacement,
                )));
            }
            if let Some(m) = regex.find(&string) {
                let start = m.start();
                let end = m.end();
                let matched = &string[start..end];
                let before = &string[..start];
                let after = &string[end..];

                let captures = match_captures(&m, &string);
                let replaced = replace_using_value(
                    matched,
                    before,
                    after,
                    &captures,
                    start as f64,
                    &string,
                    &replacement,
                )?;
                return Ok(Value::String(format!("{}{}{}", before, replaced, after)));
            }
        }
    }

    Ok(Value::String(string))
}

/// Perform replacement using a Value (either string or function)
fn replace_using_value(
    matched: &str,
    before: &str,
    after: &str,
    captures: &[&str],
    position: f64,
    string: &str,
    replacement: &Value,
) -> Result<String, JsError> {
    if matches!(
        replacement,
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
    ) {
        let args: Vec<Value> = std::iter::once(Value::String(matched.to_string()))
            .chain(captures.iter().map(|s| Value::String(s.to_string())))
            .chain([Value::Number(position), Value::String(string.to_string())])
            .collect();

        let result =
            crate::eval::call_value_with_this(replacement.clone(), args, Value::Undefined)?;
        Ok(to_js_string(&result))
    } else {
        let replacer = to_js_string(replacement);
        Ok(apply_substitution(
            &replacer, matched, before, after, captures,
        ))
    }
}

/// Replace all matches using a Value (either string or function)
fn replace_all_with_value(string: &str, regex: &Regex, replacement: &Value) -> String {
    if matches!(
        replacement,
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
    ) {
        let mut result = String::new();
        let mut last_end = 0;

        for m in regex.find_iter(string) {
            let start = m.start();
            let end = m.end();
            let matched = &string[start..end];
            let captures = match_captures(&m, string);

            let args: Vec<Value> = std::iter::once(Value::String(matched.to_string()))
                .chain(captures.iter().map(|s| Value::String(s.to_string())))
                .chain([
                    Value::Number(start as f64),
                    Value::String(string.to_string()),
                ])
                .collect();

            let func_result =
                crate::eval::call_value_with_this(replacement.clone(), args, Value::Undefined);
            let replaced = match func_result {
                Ok(val) => to_js_string(&val),
                Err(_) => matched.to_string(),
            };

            result.push_str(&string[last_end..start]);
            result.push_str(&replaced);
            last_end = end;
        }
        result.push_str(&string[last_end..]);
        result
    } else {
        let replacer = to_js_string(replacement);
        replace_all_matches(string, regex, &replacer)
    }
}

pub(crate) fn string_replace_all_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.replaceAll requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let pattern = args.first().cloned();
    let replacement = args.get(1).cloned();

    if let (Some(pattern), Some(replacement)) = (pattern, replacement) {
        return replace_all_inner(&string, &pattern, &replacement);
    }
    Ok(Value::String(string))
}

fn replace_all_inner(string: &str, pattern: &Value, replacement: &Value) -> Result<Value, JsError> {
    if let Value::Object(ref obj) = pattern {
        if obj.borrow().internal_regex.is_some() {
            let has_global = obj
                .borrow()
                .get("global")
                .map(|v| v == Value::Boolean(true))
                .unwrap_or(false);
            if !has_global {
                return Err(JsError::new(
                    "TypeError: String.prototype.replaceAll called with non-global RegExp"
                        .to_string(),
                ));
            }
        }
    }

    let pattern_str = to_js_string(pattern);

    if pattern_str.is_empty() {
        let replacer = to_js_string(replacement);
        return Ok(Value::String(replace_all_empty(string, &replacer)));
    }

    let regex = match pattern {
        Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
        _ => Regex::new(&pattern_str).ok(),
    };

    if let Some(regex) = regex {
        let result = replace_all_with_value(string, &regex, replacement);
        return Ok(Value::String(result));
    }
    Ok(Value::String(string.to_string()))
}

fn replace_all_empty(string: &str, replacer: &str) -> String {
    let mut result = String::new();
    if string.is_empty() {
        return result;
    }
    result.push_str(replacer);
    for (i, c) in string.chars().enumerate() {
        if i > 0 {
            result.push_str(replacer);
        }
        result.push(c);
    }
    result.push_str(replacer);
    result
}

pub(crate) fn string_split_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError::new("String.prototype.split requires 'this'".to_string()))?;

    let string = to_js_string(&this_val);
    let separator = args.first().cloned();

    if let Some(separator) = separator {
        let limit = args
            .get(1)
            .map(|v| to_number(v) as usize)
            .unwrap_or(usize::MAX);
        if separator == Value::String("".to_string()) {
            let chars: Vec<Value> = string
                .chars()
                .take(limit)
                .map(|c| Value::String(c.to_string()))
                .collect();
            return Ok(make_value_array(chars));
        }
        if let Some(regex) = get_separator_regex(&separator) {
            return Ok(split_by_regex(&string, &regex, limit));
        }
    }
    Ok(make_value_array(vec![Value::String(string)]))
}

fn get_separator_regex(separator: &Value) -> Option<Regex> {
    match separator {
        Value::Object(ref obj) => obj.borrow().internal_regex.clone(),
        _ => {
            let sep_str = to_js_string(separator);
            if sep_str.is_empty() {
                None
            } else {
                Regex::new(&sep_str).ok()
            }
        }
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests;
