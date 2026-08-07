//! String search methods (indexOf, lastIndexOf, includes, startsWith, endsWith)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, JsError, NativeFunction, Object, Value};

/// Clamp `pos` to `s.len()` and floor it to the nearest char boundary,
/// so byte-slicing `s` with it can never panic.
fn floor_boundary(s: &str, mut pos: usize) -> usize {
    pos = pos.min(s.len());
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

fn index_of_impl(args: &[Value], s: &str) -> Value {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
    let start = floor_boundary(s, start);
    Value::Number(
        s[start..]
            .find(&needle)
            .map(|i| (start + i) as f64)
            .unwrap_or(-1.0),
    )
}

fn last_index_of_impl(args: &[Value], s: &str) -> Value {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    let pos = args
        .get(1)
        .map(|v| to_number(v) as usize)
        .unwrap_or(usize::MAX);
    let pos = floor_boundary(s, pos);
    let result = s[..pos].rfind(&needle).map(|i| i as f64).unwrap_or(-1.0);
    Value::Number(result)
}

fn includes_impl(args: &[Value], s: &str) -> Value {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
    let start = floor_boundary(s, start);
    Value::Boolean(s[start..].contains(&needle))
}

fn starts_with_impl(args: &[Value], s: &str) -> Value {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    let start = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);
    let start = floor_boundary(s, start);
    Value::Boolean(s[start..].starts_with(&needle))
}

fn ends_with_impl(args: &[Value], s: &str) -> Value {
    let needle = args.first().map(to_js_string).unwrap_or_default();
    let end_pos = args.get(1).map(|v| to_number(v) as usize);
    let matches = if let Some(pos) = end_pos {
        s[..floor_boundary(s, pos)].ends_with(&needle)
    } else {
        s.ends_with(&needle)
    };
    Value::Boolean(matches)
}

/// Install indexOf and lastIndexOf methods
fn install_index_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "indexOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(index_of_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
    proto_clone.borrow_mut().set(
        "lastIndexOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(last_index_of_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

/// Install includes, startsWith, endsWith methods
#[allow(clippy::complexity)]
fn install_prefix_suffix_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "includes",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(includes_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
    proto_clone.borrow_mut().set(
        "startsWith",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(starts_with_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
    proto_clone.borrow_mut().set(
        "endsWith",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(ends_with_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

/// Minimal String.prototype.match: calls regexp.exec(this) and returns the match.
fn string_match(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let this_str = crate::value::to_js_string(&this_val);
    let regexp = args.first().cloned().unwrap_or(Value::Undefined);
    // Use the regex's exec method
    let exec_fn = match &regexp {
        Value::Object(obj) => obj.borrow().get("exec"),
        _ => None,
    };
    let exec_fn = match exec_fn {
        Some(f) => f,
        None => return Ok(Value::Undefined),
    };
    crate::eval::call_value_with_this(exec_fn, vec![Value::String(this_str)], regexp)
}

/// Minimal String.prototype.search: calls regexp.exec(this) and returns the index.
fn string_search(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let this_str = crate::value::to_js_string(&this_val);
    let regexp = args.first().cloned().unwrap_or(Value::Undefined);
    let exec_fn = match &regexp {
        Value::Object(obj) => obj.borrow().get("exec"),
        _ => None,
    };
    let exec_fn = match exec_fn {
        Some(f) => f,
        None => return Ok(Value::Number(-1.0)),
    };
    match crate::eval::call_value_with_this(exec_fn, vec![Value::String(this_str)], regexp) {
        Ok(Value::Null) => Ok(Value::Number(-1.0)),
        Ok(Value::Object(obj)) => Ok(obj.borrow().get("index").unwrap_or(Value::Number(-1.0))),
        _ => Ok(Value::Number(-1.0)),
    }
}

/// Install all search methods
pub fn install_search_methods(proto: &Rc<RefCell<Object>>) {
    install_index_methods(proto);
    install_prefix_suffix_methods(proto);
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "match",
        Value::NativeFunction(Rc::new(NativeFunction::new(string_match))),
    );
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "search",
        Value::NativeFunction(Rc::new(NativeFunction::new(string_search))),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_of_multibyte_start_no_panic() {
        // "é" is two bytes; start=1 is not a char boundary
        let s = "éabc";
        let args = vec![Value::String("abc".to_string()), Value::Number(1.0)];
        assert_eq!(index_of_impl(&args, s), Value::Number(2.0));
    }

    #[test]
    fn test_index_of_start_beyond_len_no_panic() {
        let s = "abc";
        let args = vec![Value::String("b".to_string()), Value::Number(100.0)];
        assert_eq!(index_of_impl(&args, s), Value::Number(-1.0));
        assert_eq!(includes_impl(&args, s), Value::Boolean(false));
        assert_eq!(starts_with_impl(&args, s), Value::Boolean(false));
    }

    #[test]
    fn test_last_index_of_multibyte_pos_no_panic() {
        let s = "éabc";
        let args = vec![Value::String("é".to_string()), Value::Number(1.0)];
        // pos=1 floors to boundary 0, so "é" is not found in ""
        assert_eq!(last_index_of_impl(&args, s), Value::Number(-1.0));
        let args = vec![Value::String("é".to_string())];
        assert_eq!(last_index_of_impl(&args, s), Value::Number(0.0));
    }

    #[test]
    fn test_ends_with_multibyte_pos_no_panic() {
        let s = "éabc";
        let args = vec![Value::String("é".to_string()), Value::Number(1.0)];
        assert_eq!(ends_with_impl(&args, s), Value::Boolean(false));
        let args = vec![Value::String("é".to_string()), Value::Number(2.0)];
        assert_eq!(ends_with_impl(&args, s), Value::Boolean(true));
    }
}
