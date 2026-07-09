//! String pad methods (padStart, padEnd)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, NativeFunction, Object, Value};

/// Install padStart method
fn install_pad_start_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set("padStart", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let target = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let pad = args.get(1).map(to_js_string).unwrap_or_else(|| " ".to_string());
                if s.len() >= target {
                    return Ok(Value::String(s.clone()));
                }
                let pad_len = target - s.len();
                let pad_count = pad_len.div_ceil(pad.len());
                let padding: String = pad.repeat(pad_count);
                Ok(Value::String(format!("{}{}", &padding[..pad_len], s)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install padEnd method
fn install_pad_end_method(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);
    proto_clone.borrow_mut().set("padEnd", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let target = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                let pad = args.get(1).map(to_js_string).unwrap_or_else(|| " ".to_string());
                if s.len() >= target {
                    return Ok(Value::String(s.clone()));
                }
                let pad_len = target - s.len();
                let pad_count = pad_len.div_ceil(pad.len());
                let padding: String = pad.repeat(pad_count);
                Ok(Value::String(format!("{}{}", s, &padding[..pad_len])))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}

/// Install repeat and pad methods
pub fn install_repeat_pad_methods(proto: &Rc<RefCell<Object>>) {
    install_pad_start_method(proto);
    install_pad_end_method(proto);
}
