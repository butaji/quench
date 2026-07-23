//! String trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeFunction, Object, Value};

fn install_trim_method(proto: &Rc<RefCell<Object>>, name: &str, trim_fn: fn(&str) -> &str) {
    proto.borrow_mut().set(
        name,
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
            let s = get_string_this_val();
            match s {
                Some(s) => Ok(Value::String(trim_fn(&s).to_string())),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

/// Extract the string value from `get_native_this()`, handling both
/// bare `Value::String` and boxed string objects (stored as `_value`).
fn get_string_this_val() -> Option<String> {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(obj)) => match obj.borrow().get("_value") {
            Some(Value::String(s)) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

/// Install trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)
pub fn install_trim_methods(proto: &Rc<RefCell<Object>>) {
    install_trim_method(proto, "trim", |s| s.trim());
    install_trim_method(proto, "trimStart", |s| s.trim_start());
    install_trim_method(proto, "trimLeft", |s| s.trim_start());
    install_trim_method(proto, "trimEnd", |s| s.trim_end());
    install_trim_method(proto, "trimRight", |s| s.trim_end());
}
