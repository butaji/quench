//! String pad methods (padStart, padEnd)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, NativeFunction, Object, Value};

/// Shared pad implementation: builds `pad_len` chars of padding from `pad`
/// by cycling its chars. Returns the original string for an empty pad
/// (spec: padString "" means no padding is added).
fn pad_impl(s: &str, target: usize, pad: &str, at_start: bool) -> Value {
    if s.len() >= target || pad.is_empty() {
        return Value::String(s.to_string());
    }
    let pad_len = target - s.len();
    let padding: String = pad.chars().cycle().take(pad_len).collect();
    if at_start {
        Value::String(format!("{}{}", padding, s))
    } else {
        Value::String(format!("{}{}", s, padding))
    }
}

/// Install padStart method
fn install_pad_start_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "padStart",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => {
                    let target = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                    let pad = args
                        .get(1)
                        .map(to_js_string)
                        .unwrap_or_else(|| " ".to_string());
                    Ok(pad_impl(&s, target, &pad, true))
                }
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

/// Install padEnd method
fn install_pad_end_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "padEnd",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => {
                    let target = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                    let pad = args
                        .get(1)
                        .map(to_js_string)
                        .unwrap_or_else(|| " ".to_string());
                    Ok(pad_impl(&s, target, &pad, false))
                }
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

/// Install repeat and pad methods
pub fn install_repeat_pad_methods(proto: &Rc<RefCell<Object>>) {
    install_pad_start_method(proto);
    install_pad_end_method(proto);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_empty_pad_string_returns_original() {
        // Empty pad previously panicked with division by zero
        assert_eq!(pad_impl("a", 5, "", true), Value::String("a".to_string()));
        assert_eq!(pad_impl("a", 5, "", false), Value::String("a".to_string()));
    }

    #[test]
    fn test_pad_multibyte_pad_no_panic() {
        // Multibyte pad previously panicked on byte-slicing the padding
        assert_eq!(
            pad_impl("a", 4, "é", true),
            Value::String("éééa".to_string())
        );
        assert_eq!(
            pad_impl("a", 4, "é", false),
            Value::String("aééé".to_string())
        );
    }

    #[test]
    fn test_pad_basic() {
        assert_eq!(
            pad_impl("a", 3, " ", true),
            Value::String("  a".to_string())
        );
        assert_eq!(
            pad_impl("a", 3, "0", false),
            Value::String("a00".to_string())
        );
        assert_eq!(
            pad_impl("abc", 2, " ", true),
            Value::String("abc".to_string())
        );
    }
}
