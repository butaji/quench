//! String toString methods (toString, valueOf)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, NativeFunction, Object, Value};

/// Install toString and valueOf methods
pub fn install_to_string_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.clone())),
            Some(v) => Ok(Value::String(to_js_string(&v))),
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("valueOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => Ok(Value::String(s.clone())),
            Some(v) => Ok(Value::String(to_js_string(&v))),
            _ => Ok(Value::Undefined),
        }
    }))));
}
