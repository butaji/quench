//! String method implementations for the stack machine.
//!
//! This module contains helper functions for evaluating string methods.

use std::rc::Rc;
use std::cell::RefCell;

use crate::value::{Value, JsError, Object, NativeFunction, to_js_string, to_number};

/// Read a property from a string value.
pub fn read_string_property(s: &str, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "length" => string_length(s),
        "charAt" => string_char_at(s),
        "charCodeAt" => string_char_code_at(s),
        "indexOf" => string_index_of(s),
        "toUpperCase" => Ok(Value::String(s.to_uppercase())),
        "toLowerCase" => Ok(Value::String(s.to_lowercase())),
        "trim" => Ok(Value::String(s.trim().to_string())),
        "includes" => string_includes(s),
        "startsWith" => string_starts_with(s),
        "endsWith" => string_ends_with(s),
        "concat" => string_concat(s),
        "split" => string_split(s),
        "substring" => string_substring(s),
        "slice" => string_slice(s),
        "match" => string_match(s),
        "search" => string_search(s),
        _ => Ok(Value::Undefined),
    }
}

/// Create a native function wrapper for string methods.
pub fn make_string_method(s: &str, method_name: &str) -> Value {
    let s_clone = s.to_string();
    let method_clone = method_name.to_string();
    Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        read_string_method(&s_clone, &method_clone, args)
    })))
}

fn read_string_method(s: &str, method_name: &str, args: Vec<Value>) -> Result<Value, JsError> {
    match method_name {
        "charAt" => string_char_at_impl(s, &args),
        "charCodeAt" => string_char_code_at_impl(s, &args),
        "indexOf" => string_index_of_impl(s, &args),
        "includes" => string_includes_impl(s, &args),
        "startsWith" => string_starts_with_impl(s, &args),
        "endsWith" => string_ends_with_impl(s, &args),
        "concat" => string_concat_impl(s, &args),
        "split" => string_split_impl(s, &args),
        "substring" => string_substring_impl(s, &args),
        "slice" => string_slice_impl(s, &args),
        "match" => string_match_impl(s, &args),
        "search" => string_search_impl(s, &args),
        _ => Ok(Value::Undefined),
    }
}

fn string_length(s: &str) -> Result<Value, JsError> {
    Ok(Value::Number(s.len() as f64))
}

fn string_char_at(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_char_at_impl(&s_clone, &args)
    }))))
}

fn string_char_at_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
}

fn string_char_code_at(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_char_code_at_impl(&s_clone, &args)
    }))))
}

fn string_char_code_at_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    Ok(Value::Number(s.chars().nth(idx).map(|c| c as u32 as f64).unwrap_or(f64::NAN)))
}

fn string_index_of(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_index_of_impl(&s_clone, &args)
    }))))
}

fn string_index_of_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
}

fn string_includes(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_includes_impl(&s_clone, &args)
    }))))
}

fn string_includes_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    Ok(Value::Boolean(s.contains(&needle)))
}

fn string_starts_with(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_starts_with_impl(&s_clone, &args)
    }))))
}

fn string_starts_with_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    Ok(Value::Boolean(s.starts_with(&needle)))
}

fn string_ends_with(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_ends_with_impl(&s_clone, &args)
    }))))
}

fn string_ends_with_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    Ok(Value::Boolean(s.ends_with(&needle)))
}

fn string_concat(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_concat_impl(&s_clone, &args)
    }))))
}

fn string_concat_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let sep = args.iter().map(to_js_string).collect::<Vec<_>>().join("");
    Ok(Value::String(format!("{}{}", s, sep)))
}

fn string_split(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_split_impl(&s_clone, &args)
    }))))
}

fn string_split_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let sep = args.first().map(to_js_string).unwrap_or_default();
    let parts: Vec<Value> = if sep.is_empty() {
        s.chars().map(|c| Value::String(c.to_string())).collect()
    } else {
        s.split(&sep).map(|p| Value::String(p.to_string())).collect()
    };
    Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(parts.len())))))
}

fn string_substring(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_substring_impl(&s_clone, &args)
    }))))
}

fn string_substring_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
    let start = start.min(s.len());
    let end = end.min(s.len());
    let start = start.min(end);
    Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
}

fn string_slice(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_slice_impl(&s_clone, &args)
    }))))
}

fn string_slice_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0) as isize;
    let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
    let len = s.len() as isize;
    let start = if start < 0 { (len + start).max(0) as usize } else { start as usize }.min(len as usize);
    let end = if end < 0 { (len + end).max(0) as usize } else { end as usize }.min(len as usize);
    let end = end.max(start);
    Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
}

fn string_match(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_match_impl(&s_clone, &args)
    }))))
}

fn string_match_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let pattern = args.first().map(to_js_string).unwrap_or_default();
    Ok(Value::Boolean(s.contains(&pattern)))
}

fn string_search(s: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        string_search_impl(&s_clone, &args)
    }))))
}

fn string_search_impl(s: &str, args: &[Value]) -> Result<Value, JsError> {
    let pattern = args.first().map(to_js_string).unwrap_or_default();
    Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
}
