//! String repeat method

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

pub fn install_split_concat_methods(proto: &Rc<RefCell<Object>>) {
    proto.borrow_mut().set(
        "__repeat",
        Value::NativeFunction(Rc::new(NativeFunction::new(string_repeat_impl))),
    );
}

fn string_repeat_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    match super::this_js_string() {
        Some(s) => {
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
