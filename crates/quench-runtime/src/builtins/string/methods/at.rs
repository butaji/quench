//! String.prototype.at - returns character at index, negative = from end

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, JsError, NativeFunction, Object, Value};

/// Install String.prototype.at method
pub fn install_at_method(proto: &Rc<RefCell<Object>>) {
    proto.borrow_mut().set(
        "at",
        Value::NativeFunction(Rc::new(NativeFunction::new(proto_at))),
    );
}

/// String.prototype.at(index) - returns character at index, negative = from end
fn proto_at(args: Vec<Value>) -> Result<Value, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::String(s)) => {
            let len = s.chars().count() as f64;
            let idx = args.first().map(to_number).unwrap_or(0.0);

            let actual_idx = if idx < 0.0 {
                (len + idx) as isize
            } else {
                idx as isize
            };

            if actual_idx < 0 || (actual_idx as usize) >= s.chars().count() {
                Ok(Value::Undefined)
            } else {
                let ch = s.chars().nth(actual_idx as usize).map(|c| c.to_string());
                Ok(ch.map(Value::String).unwrap_or(Value::Undefined))
            }
        }
        _ => Ok(Value::Undefined),
    }
}
