//! String concat methods (split, concat, repeat)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, NativeFunction, Object, Value};

pub fn install_split_concat_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "split",
        Value::NativeFunction(Rc::new(NativeFunction::new(string_split_impl))),
    );
    proto_clone.borrow_mut().set(
        "concat",
        Value::NativeFunction(Rc::new(NativeFunction::new(string_concat_impl))),
    );

    let proto_clone2 = Rc::clone(proto);
    proto_clone2.borrow_mut().set(
        "repeat",
        Value::NativeFunction(Rc::new(NativeFunction::new(string_repeat_impl))),
    );
}

fn string_split_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => {
            let sep = args.first().map(to_js_string).unwrap_or_default();
            let limit = args.get(1).map(|v| to_number(v) as usize);
            let parts: Vec<Value> = if sep.is_empty() {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            } else {
                s.split(&sep)
                    .map(|p| Value::String(p.to_string()))
                    .collect()
            };
            let parts = if let Some(l) = limit {
                parts.into_iter().take(l).collect()
            } else {
                parts
            };
            let arr = Object::new_array_from(parts);
            Ok(Value::Object(Rc::new(RefCell::new(arr))))
        }
        _ => Ok(Value::Undefined),
    }
}

fn string_concat_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => {
            let rest: String = args.iter().map(to_js_string).collect();
            Ok(Value::String(format!("{}{}", s, rest)))
        }
        _ => Ok(Value::Undefined),
    }
}

fn string_repeat_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => {
            let count = args.first().map(to_number).unwrap_or(0.0);
            if count < 0.0 || count.is_infinite() {
                return Err(crate::JsError::new(
                    "RangeError: Invalid count value".to_string(),
                ));
            }
            let count = count as usize;
            // Cap the result length to avoid OOM on huge counts
            if s.len().saturating_mul(count) > (1 << 24) {
                return Err(crate::JsError::new(
                    "RangeError: Invalid string length".to_string(),
                ));
            }
            Ok(Value::String(s.repeat(count)))
        }
        _ => Ok(Value::Undefined),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    #[test]
    fn test_repeat_negative_count_throws_range_error() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("\"abc\".repeat(-1)");
        assert!(result.is_err(), "repeat(-1) must throw RangeError");
        assert!(result.unwrap_err().0.contains("RangeError"));
    }

    #[test]
    fn test_repeat_huge_count_throws_range_error() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("\"abc\".repeat(1e9)");
        assert!(
            result.is_err(),
            "repeat with huge count must throw RangeError"
        );
    }

    #[test]
    fn test_repeat_valid_count() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("\"abc\".repeat(3)").unwrap();
        assert_eq!(result, Value::String("abcabcabc".to_string()));
    }
}
