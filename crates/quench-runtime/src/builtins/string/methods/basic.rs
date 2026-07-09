//! Basic string methods (length, charAt, charCodeAt)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};

/// Install basic string methods (length, charAt, charCodeAt)
pub fn install_basic_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("length", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::Number(s.len() as f64)),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("charAt", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("charCodeAt", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let code = s.chars().nth(idx).map(|c| c as u16 as f64).unwrap_or(f64::NAN);
                Ok(Value::Number(code))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}
