//! Basic string methods (length, charAt, charCodeAt)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

fn string_length_impl(_args: &[Value]) -> Value {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => Value::Number(s.len() as f64),
        _ => Value::Undefined,
    }
}

fn char_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    Value::String(
        s.chars()
            .nth(idx)
            .map(|c| c.to_string())
            .unwrap_or_default(),
    )
}

fn char_code_at_impl(args: &[Value], s: &str) -> Value {
    let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
    Value::Number(
        s.chars()
            .nth(idx)
            .map(|c| c as u16 as f64)
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
