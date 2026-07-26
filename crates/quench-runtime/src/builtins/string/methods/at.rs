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

/// String.prototype.at(index) - returns UTF-16 code unit at index, negative = from end
fn proto_at(args: Vec<Value>) -> Result<Value, JsError> {
    match super::this_js_string() {
        Some(s) => {
            let utf16: Vec<u16> = s.encode_utf16().collect();
            let len = utf16.len() as f64;
            let idx = args.first().map(to_number).unwrap_or(0.0);

            let actual_idx = if idx < 0.0 {
                (len + idx) as isize
            } else {
                idx as isize
            };

            if actual_idx < 0 || (actual_idx as usize) >= utf16.len() {
                Ok(Value::Undefined)
            } else {
                let code_unit = utf16[actual_idx as usize];
                // Convert single UTF-16 code unit to String
                Ok(Value::String(
                    String::from_utf16(&[code_unit]).unwrap_or_else(|_| String::new()),
                ))
            }
        }
        _ => Ok(Value::Undefined),
    }
}
