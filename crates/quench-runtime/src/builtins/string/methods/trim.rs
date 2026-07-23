//! String trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)

use std::cell::RefCell;
use std::rc::Rc;

use super::this_js_string;
use crate::value::{NativeFunction, Object, Value};

fn install_trim_method(proto: &Rc<RefCell<Object>>, name: &str, trim_fn: fn(&str) -> &str) {
    proto.borrow_mut().set(
        name,
        Value::NativeFunction(Rc::new(NativeFunction::new(
            move |_| match this_js_string() {
                Some(s) => Ok(Value::String(trim_fn(&s).to_string())),
                None => Ok(Value::Undefined),
            },
        ))),
    );
}

/// Install trim methods (trim, trimStart, trimLeft, trimEnd, trimRight)
pub fn install_trim_methods(proto: &Rc<RefCell<Object>>) {
    install_trim_method(proto, "trim", |s| s.trim());
    install_trim_method(proto, "trimStart", |s| s.trim_start());
    install_trim_method(proto, "trimLeft", |s| s.trim_start());
    install_trim_method(proto, "trimEnd", |s| s.trim_end());
    install_trim_method(proto, "trimRight", |s| s.trim_end());
}
