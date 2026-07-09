//! String trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeFunction, Object, Value};

/// Install trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)
pub fn install_trim_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("trim", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimStart", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_start().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimLeft", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_start().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimEnd", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_end().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("trimRight", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.trim_end().to_string())),
            _ => Ok(Value::Undefined),
        }
    }))));
}
