//! String concat methods (split, concat, repeat)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, NativeFunction, Object, Value};

/// Install split and concat methods
pub fn install_split_concat_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("split", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let sep = args.first().map(to_js_string).unwrap_or_default();
                let limit = args.get(1).map(|v| to_number(v) as usize);
                let parts: Vec<Value> = if sep.is_empty() {
                    s.chars().map(|c| Value::String(c.to_string())).collect()
                } else {
                    s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                };
                let parts = if let Some(l) = limit {
                    parts.into_iter().take(l).collect()
                } else {
                    parts
                };
                let arr = Object::new_array_from(parts);
                Ok(Value::Object(Rc::new(RefCell::new(arr))))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("concat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let rest: String = args.iter().map(to_js_string).collect();
                Ok(Value::String(format!("{}{}", s, rest)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    // repeat is installed separately in pad module
    let proto_clone2 = Rc::clone(proto);
    proto_clone2.borrow_mut().set("repeat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let count = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                Ok(Value::String(s.repeat(count)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}
