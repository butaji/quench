//! String toString methods (toString, valueOf)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, NativeFunction, Object, Value};

fn string_value_impl(_args: &[Value]) -> Value {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Object(obj)) => match obj.borrow().get("_value") {
            Some(Value::String(s)) => Value::String(s),
            _ => Value::String("[object Object]".to_string()),
        },
        Some(v) => Value::String(to_js_string(&v)),
        _ => Value::Undefined,
    }
}

/// Install toString and valueOf methods
pub fn install_to_string_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            Ok(string_value_impl(&args))
        }))),
    );
    proto_clone.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
            Ok(string_value_impl(&args))
        }))),
    );
}
