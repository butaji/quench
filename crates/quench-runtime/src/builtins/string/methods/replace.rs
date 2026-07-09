//! String replace methods (replace, match, search)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, NativeFunction, Object, Value};

/// Install replace, match, search methods
pub fn install_replace_match_methods(proto: &Rc<RefCell<Object>>) {
    let proto_clone = Rc::clone(proto);

    proto_clone.borrow_mut().set("replace", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let search = args.first().map(to_js_string).unwrap_or_default();
                let replace = args.get(1).map(to_js_string).unwrap_or_default();
                let new_s = if let Some(pos) = s.find(&search) {
                    format!("{}{}{}", &s[..pos], replace, &s[pos + search.len()..])
                } else {
                    s.clone()
                };
                Ok(Value::String(new_s))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("match", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let pattern = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Boolean(s.contains(&pattern)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));

    proto_clone.borrow_mut().set("search", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        match crate::builtins::get_native_this() {
            Some(Value::String(s)) => {
                let pattern = args.first().map(to_js_string).unwrap_or_default();
                Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
            }
            _ => Ok(Value::Undefined),
        }
    }))));
}
