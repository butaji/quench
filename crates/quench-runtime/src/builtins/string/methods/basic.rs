//! Basic string methods (length, charAt, charCodeAt)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

fn string_length_impl(_args: &[Value]) -> Value {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => Value::Number(s.encode_utf16().count() as f64),
        _ => Value::Undefined,
    }
}

fn char_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    // ES spec §21.1.3.1: charAt returns a string of length 1 (one UTF-16 code unit).
    // For surrogate pairs, this returns the individual surrogate code unit as a string.
    Value::String(
        s.encode_utf16()
            .nth(idx)
            .map(|cu| {
                // Convert the UTF-16 code unit to a Rust char, or to a placeholder
                std::char::from_u32(cu as u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| format!("\\u{:04X}", cu))
            })
            .unwrap_or_default(),
    )
}

fn char_code_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    // ES spec §21.1.3.2: charCodeAt returns UTF-16 code unit at index,
    // treating supplementary characters as two surrogate code units.
    Value::Number(
        s.encode_utf16()
            .nth(idx)
            .map(|cu| cu as f64)
            .unwrap_or(f64::NAN),
    )
}

/// Install basic string methods (length, charAt, charCodeAt)
pub fn install_basic_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "length",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            Ok(string_length_impl(&args))
        }))),
    );
    proto_clone.borrow_mut().set(
        "charAt",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(char_at_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
    proto_clone.borrow_mut().set(
        "charCodeAt",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(char_code_at_impl(&args, &s)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}
