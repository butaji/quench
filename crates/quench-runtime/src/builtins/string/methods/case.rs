//! String case methods (toUpperCase, toLowerCase)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeFunction, Object, Value};

/// Install case methods (toUpperCase, toLowerCase)
pub fn install_case_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set(
        "toUpperCase",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(Value::String(s.to_uppercase())),
                _ => Ok(Value::Undefined),
            }
        }))),
    );

    proto_clone.borrow_mut().set(
        "toLowerCase",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
            match crate::builtins::get_native_this() {
                Some(Value::String(s)) => Ok(Value::String(s.to_lowercase())),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}
